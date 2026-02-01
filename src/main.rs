#![no_std]
#![no_main]
use embedded_hal::delay::DelayNs;
use rp235x_hal::{self as hal, Clock};
// For SPI
use embassy_rp::spi;
use embassy_rp::spi::Spi;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

// For CS Pin
use embassy_rp::gpio::{Level, Output};

// For SdCard
use embedded_sdmmc::{SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};

use panic_halt as _;
use rp235x_hal::block::ImageDef;
use rp235x_hal::fugit::RateExtU32;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};
use usbd_serial::SerialPort;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = hal::block::ImageDef::secure_exe();

pub const XTAL_FREQ_HZ: u32 = 12_000_000u32;

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
    let mut pac = hal::pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

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
    let mut timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    let sio = hal::Sio::new(pac.SIO);

    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );
    let spi_cs = pins.gpio1.into_push_pull_output();
    let spi_sck = pins.gpio2.into_function::<hal::gpio::FunctionSpi>();
    let spi_mosi = pins.gpio3.into_function::<hal::gpio::FunctionSpi>();
    let spi_miso = pins.gpio4.into_function::<hal::gpio::FunctionSpi>();
    let spi_bus = hal::spi::Spi::<_, _, _, 8>::new(pac.SPI0, (spi_mosi, spi_miso, spi_sck));

    let spi = spi_bus.init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        400.kHz(), // card initialization happens at low baud rate
        embedded_hal::spi::MODE_0,
    );
    let spi = ExclusiveDevice::new(spi, spi_cs, timer).unwrap();

    let mut config = spi::Config::default();
    config.frequency = 400_000;

    let sdcard = SdCard::new(spi, Delay);

    //Read SD card size to verify initialization
    log::info!("Init SD card controller and retrieve card size...");
    let sd_size = sdcard.num_bytes().expect("failed to get sdcard size");
    log::info!("card size is {} bytes", sd_size);


    let mut volume_mgr = VolumeManager::new(sdcard, DummyTimesource::default());
    let mut volume0 = volume_mgr
        .open_volume(VolumeIdx(0))
        .expect("failed to open volume");

    let mut root_dir = volume0.open_root_dir().expect("failed to open root dir");

    //Get the file to open for reading. In this case, it is RUST.TXT
    let mut read_file = root_dir
        .open_file_in_dir("RUST.TXT", embedded_sdmmc::Mode::ReadOnly)
        .expect("failed to open RUST.TXT file");

    let usb_bus = UsbBusAllocator::new(hal::usb::UsbBus::new(
        pac.USB,
        pac.USB_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut serial = SerialPort::new(&usb_bus);

    /*let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("implRust")
            .product("Ferris")
            .serial_number("TEST")])
        .unwrap()
        .device_class(2) // 2 for the CDC, from: https://www.usb.org/defined-class-codes
        .build();*/

    loop {
        while !read_file.is_eof() {           //Loop until the end of the file
            break; //Skip for now
            let mut buffer = [0u8; 32]; //Print size

            if let Ok(n) = read_file.read(&mut buffer) {
                if let Ok(s) = core::str::from_utf8(&buffer[..n]) {
                    serial.write(s.as_bytes()).unwrap(); //Output string s from buffer
                } else {
                    serial.write(&buffer[..n]).unwrap(); //Output final string from buffer
                }
            }
        }
        serial.write("Loop!".as_bytes()).unwrap();

        timer.delay_ms(50);
    }
}
