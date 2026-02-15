#![no_std]
#![no_main]

use rp235x_hal as hal;
use hal::block::ImageDef;
use rp235x_hal::{spi, Clock};
use embedded_hal::digital::OutputPin;
use {panic_probe as _};
use defmt_rtt as _;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use rp235x_hal::fugit::RateExtU32;
use heapless::String;
use core::fmt::Write;
use rp235x_hal::reboot::{reboot, RebootKind, RebootArch};

/// Tell the Boot ROM about our application
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = hal::block::ImageDef::secure_exe();
/// External high-speed crystal on the Raspberry Pi Pico 2 board is 12 MHz.
/// Adjust if your board has a different frequency
const XTAL_FREQ_HZ: u32 = 12_000_000u32;
// USB Device support
use usb_device::{class_prelude::*, prelude::*};
// USB Communications Class Device support
use usbd_serial::SerialPort;




/// Code from https://github.com/rp-rs/rp-hal-boards/blob/main/boards/rp-pico/examples/pico_spi_sd_card.rs
/// A dummy timesource, which is mostly important for creating files.
#[derive(Default)]
pub struct DummyTimesource();

impl TimeSource for DummyTimesource {
    // In theory you could use the RTC of the rp2040 here, if you had
    // any external time synchronizing device.
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}


#[hal::entry]
fn main() -> ! {
    // Grab our singleton objects
    let mut pac = hal::pac::Peripherals::take().unwrap();

    // Set up the watchdog driver - needed by the clock setup code
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    // Configure the clocks
    //
    // The default is to generate a 125 MHz system clock
    let clocks = hal::clocks::init_clocks_and_plls(
        XTAL_FREQ_HZ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
        .ok()
        .unwrap();

    // The single-cycle I/O block controls our GPIO pins
    let sio = hal::Sio::new(pac.SIO);

    // Set the pins up according to their function on this particular board
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);
    let mut timer_sd = timer;

    // Creating a usb bus to use
    let usb_bus = UsbBusAllocator::new(hal::usb::UsbBus::new(
        pac.USB,
        pac.USB_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    // Creating a Serial port on top of the usb bus
    let mut serial = SerialPort::new(&usb_bus);

    //Set the LED Pin
    let mut led_pin = pins.gpio25.into_push_pull_output();

    // Creating the device based on the serial port based on the usb bus
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("implRust")
            .product("Ferris")
            .serial_number("TEST")])
        .unwrap()
        .device_class(2) // 2 for the CDC, from: https://www.usb.org/defined-class-codes
        .build();

    //Give USB device time to initialize before use
    for _ in 0..50_000 {
        usb_dev.poll(&mut [&mut serial]);
    }

    let spi_cs = pins.gpio17.into_push_pull_output();
    let spi_sck = pins.gpio18.into_function::<hal::gpio::FunctionSpi>();
    let spi_mosi = pins.gpio19.into_function::<hal::gpio::FunctionSpi>();
    let spi_miso = pins.gpio16.into_function::<hal::gpio::FunctionSpi>();
    let spi_bus = hal::spi::Spi::<_, _, _, 8>::new(pac.SPI0, (spi_mosi, spi_miso, spi_sck));

    let spi = spi_bus.init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        400.kHz(), // card initialization happens at low baud rate
        embedded_hal::spi::MODE_0,
    );
    let spi = ExclusiveDevice::new(spi, spi_cs, &mut timer).unwrap();

    let sdcard = SdCard::new(spi, &mut timer_sd);


    let sd_size = match sdcard.num_bytes() {
        Ok(size) => size,
        Err(e) => {
            loop
            {
                let mut debug_message: String<128> = String::new();
                let _ = write!(debug_message, "ERROR! {:?}\n\r", e);
                let _ = serial.write(debug_message.as_bytes());

                if usb_dev.poll(&mut [&mut serial]) {
                    let mut buf = [0u8; 65];
                    if let Ok(count) = serial.read(&mut buf) {
                        for &byte in &buf[..count] {
                            if byte == b'b' {
                                reboot(RebootKind::BootSel {picoboot_disabled: false, msd_disabled: false}, RebootArch::Arm);
                            }
                        }
                    }
                }
            }
        }
    };

    let mut volume_mgr = VolumeManager::new(sdcard, DummyTimesource::default());
    // Now the program hangs indefinitely on open, but the com port is readable. This specific line halts the program.
    let mut volume0 = match volume_mgr.open_volume(VolumeIdx(0))
    {
        Ok(vol) => vol,
        Err(e) => {
            loop {
                let mut debug_message: String<128> = String::new();
                let _ = write!(debug_message, "ERROR! {:?}\n\r", e);
                let _ = serial.write(debug_message.as_bytes());

                if usb_dev.poll(&mut [&mut serial]) {
                    let mut buf = [0u8; 65];
                    if let Ok(count) = serial.read(&mut buf) {
                        for &byte in &buf[..count] {
                            if byte == b'b' {
                                reboot(RebootKind::BootSel {picoboot_disabled: false, msd_disabled: false}, RebootArch::Arm);
                            }
                        }
                    }
                }
            }
        }
    };
    led_pin.set_high().unwrap(); //Turn on LED if the program reaches this point

    let mut root_dir = volume0.open_root_dir().expect("failed to open root dir");

    let mut my_file = root_dir
        .open_file_in_dir("RUST.TXT", embedded_sdmmc::Mode::ReadOnly)
        .expect("failed to open RUST.TXT file");

    let mut ticks = 0;

    loop{
        usb_dev.poll(&mut [&mut serial]);

        // Updating the ticks
        ticks += 1;

        // Responding if it hasn't said hello
        if ticks > 1_000_000 {
            // Writes bytes from `data` into the port and returns the number of bytes written.
            let _ = serial.write(b"Hello, Rust!\r\n");
            ticks -= 1_000_000;
        }

        if !my_file.is_eof() && serial.dtr() {
            let mut buffer = [0u8; 32];

            if let Ok(n) = my_file.read(&mut buffer) {
                if let Ok(s) = core::str::from_utf8(&buffer[..n]) {
                    serial.write(s.as_bytes()).unwrap();
                } else {
                    serial.write(&buffer[..n]).unwrap();
                }
            }
        }
    }
}