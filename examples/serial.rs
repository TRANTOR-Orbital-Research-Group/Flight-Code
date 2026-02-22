#![no_std]
#![no_main]

use rp235x_hal as hal;
use hal::block::ImageDef;
use rp235x_hal::spi;
use embedded_hal::digital::OutputPin;
use {panic_probe as _};
use defmt_rtt as _;
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

// impl TimeSource for DummyTimesource {
//     // In theory you could use the RTC of the rp2040 here, if you had
//     // any external time synchronizing device.
//     fn get_timestamp(&self) -> Timestamp {
//         Timestamp {
//             year_since_1970: 0,
//             zero_indexed_month: 0,
//             zero_indexed_day: 0,
//             hours: 0,
//             minutes: 0,
//             seconds: 0,
//         }
//     }
// }


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



    // Getting the LED pin ready 
    let mut led = pins.gpio25.into_push_pull_output();

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

    // Creating the device based on the serial port based on the usb bus
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
    .strings(&[StringDescriptors::default()
        .manufacturer("implRust")
        .product("Ferris")
        .serial_number("TEST")])
    .unwrap()
    .device_class(2) // 2 for the CDC, from: https://www.usb.org/defined-class-codes
    .build();

    // Keeping track of timing so that we don't spam too much
    let mut ticks = 0;
    

    loop{

        // Updating the ticks 
        ticks += 1;

        // Responding if it hasn't said hello
        if ticks > 1_000_000 {
            // Writes bytes from `data` into the port and returns the number of bytes written.
            let _ = serial.write(b"Hello, Rust!\r\n");
            ticks -= 1_000_00;
        }

        if usb_dev.poll(&mut [&mut serial]) {
            let mut buf = [0u8; 64];
            if let Ok(count) = serial.read(&mut buf) {
                for &byte in &buf[..count] {
                    if byte == b'r' {
                        led.set_high().expect("unable to turn on LED");
                    } else {
                        led.set_low().expect("unable to turn off LED");
                    }
                }
            }
        }

    }
}
