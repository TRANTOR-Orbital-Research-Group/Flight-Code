
#![no_std]
#![no_main]

use rp235x_hal::{self as hal, gpio::Pins, i2c::I2C, Sio, Timer};
use {panic_probe as _};
use embedded_hal::digital::OutputPin;
use defmt_rtt as _;
use rp235x_hal::reboot::{reboot, RebootKind, RebootArch};
use fugit::RateExtU32;
use heapless::String;
use core::fmt::Write;
use embedded_hal::i2c::I2c;
use hal::prelude::*;


// Tells the Rust where to put the actual image (I think) 
use hal::block::ImageDef;
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = hal::block::ImageDef::secure_exe();

// This is where the actual RTIC application starts. We see the device as our hal's peripheral access crate and the dispacter,
// which is the interrupt vector for the software tasks. This means that all of our software interrupts use the UART0_IRQ interrupt vector
// which means that they all have a priority 2 for RTIC. We can (and likely will) add more dispatchers later so that we can have different 
// priorities for all of our different software tasks
#[rtic::app(device = hal::pac, dispatchers = [UART0_IRQ])]
mod app {
    use super::*;
    use rp235x_hal::{pac::{I2C0, otp_data::key1_3}, timer::CopyableTimer0};
    use usb_device::{class_prelude::*, prelude::*};
    use usbd_serial::SerialPort;

    // Where you put the shared resources (we don't have to name the struct shared, that was just convienent(we could have named it Steven).)
    // We can have as many of these as we want, we just have to name them different things and make sure we say that they are all shared
    #[shared]
    struct Shared {
        // Resources shared between different tasks (none right now, because we don't have any other tasks)
    }

    // Same thing as shared but it is only going to be in one task (These will belong to the idle task)
    #[local]
    struct Local {
        led: hal::gpio::Pin<hal::gpio::bank0::Gpio25, hal::gpio::FunctionSioOutput, hal::gpio::PullDown>,
        timer: hal::Timer<hal::timer::CopyableTimer0>,
        usb_dev: UsbDevice<'static, hal::usb::UsbBus>,
        serial: SerialPort<'static, hal::usb::UsbBus>,

        // Now we are just passing the I2C itself to read directly for the GPS
        // This should all be abstracted away later 
        i2c: rp235x_hal::I2C<
                rp235x_hal::pac::I2C1,
                (
                    rp235x_hal::gpio::Pin<
                        rp235x_hal::gpio::bank0::Gpio18,
                        rp235x_hal::gpio::FunctionI2c,
                        rp235x_hal::gpio::PullUp,
                    >,
                    rp235x_hal::gpio::Pin<
                        rp235x_hal::gpio::bank0::Gpio19,
                        rp235x_hal::gpio::FunctionI2c,
                        rp235x_hal::gpio::PullUp,
                    >,
                ),
            >
        // -------------------------End added stuff-------------------------

    }


    // This is the init task, which is a lot like the `void setup()` function in Arduino cpp
    // Note that it creates the Shared and Local structs that our tasks get to use
    #[init(local = [usb_bus: Option<UsbBusAllocator<hal::usb::UsbBus>> = None, gps_raw_buffer: [u8; 256] = [0u8; 256]])]
    fn init(cx: init::Context) -> (Shared, Local) {

        // All of the peripherals are off when the pico powers on, so we need the resets controller to be able to turn them on
        // That is what this is
        let mut resets = cx.device.RESETS;

        // This is the builtin watchdog. We will likely use it for flights, but for now we need it just so that we can use the clock
        let mut watchdog = hal::Watchdog::new(cx.device.WATCHDOG);
        
        // Just what it sounds like, the clock of the pico
        let clocks = hal::clocks::init_clocks_and_plls(
            12_000_000u32,
            cx.device.XOSC,
            cx.device.CLOCKS,
            cx.device.PLL_SYS,
            cx.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        ).ok().unwrap();

        // This is the Single cycle Input Output devices, which are basically just some fast gpio pins that connect close to the 
        // cpu as best as I can figure. We use them to create the pins just down a few lines
        let sio = hal::Sio::new(cx.device.SIO);

        // The struct that holds all of the - you guessed it! - pins
        let pins = hal::gpio::Pins::new(cx.device.IO_BANK0, cx.device.PADS_BANK0, sio.gpio_bank0, &mut resets);

        // The led that we want to play with
        let mut led = pins.gpio25.into_push_pull_output();

        // The timer that we need in our Local struct for our idle task
        let mut timer = hal::Timer::new_timer0(cx.device.TIMER0, &mut resets, &clocks);

        // Initializing the usb bus so that we can make a device that uses the bus for our Local struct
        let usb_bus_alloc = cx.local.usb_bus.insert(UsbBusAllocator::new(
            hal::usb::UsbBus::new(cx.device.USB, cx.device.USB_DPRAM, clocks.usb_clock, true, &mut resets)
        ));

        // Creating a serial port on the bus
        let serial = SerialPort::new(usb_bus_alloc);

        // Creating a serial device that uses that port and that bus
        let usb_dev = UsbDeviceBuilder::new(usb_bus_alloc, UsbVidPid(0x16c0, 0x27dd))
            .strings(&[StringDescriptors::default().product("RTIC Serial")])
            .unwrap()
            .device_class(2)
            .build();

        // Creating a new i2c bus on pins 18 and 19
        let i2c = I2C::i2c1(
                cx.device.I2C1,
                pins.gpio18.reconfigure(), // sda
                pins.gpio19.reconfigure(), // scl
                10.kHz(),
                &mut resets,
                clocks.peripheral_clock.freq(),
                //125_000_000.Hz(),
        );
                        
        
        // Returning our two structs
     (Shared {}, Local { led, timer, usb_dev, serial, i2c })
    }



    // This is the idle loop. The idle loop is the basically the `void loop()` part of C++ Arduino
    // The thing above it is a flag that tells Rust what it will have in scope; currently we just have 
    // a local set of variables because we don't need any shared variables right now
    // It takes in a context, which is how you access all of the variables in local and shared.
    #[idle(shared = [], local = [ led, timer, usb_dev, serial, i2c ])]
    fn idle(cx: idle::Context) -> ! {
        let mut last_scan = cx.local.timer.get_counter();
        let scan_interval = fugit::MicrosDurationU64::micros(2_500_000); 

        loop {
            // Standard USB polling
            if cx.local.usb_dev.poll(&mut [cx.local.serial]) {

                // If we do have stuff to play with, we create a buffer where the serial object can put the information in it
                let mut buf = [0u8; 64];

                // Now we try to read the buffer from the serial object
                if let Ok(count) = cx.local.serial.read(&mut buf) {

                    // Then we just iterate through the buffer to see if the key 'r' shows up in it in binary 
                    for &byte in &buf[..count] {
                        if byte == b'l' { 
                            let _ = cx.local.led.set_high(); 
                        } else if byte == b'b' { 
                            reboot(RebootKind::BootSel {picoboot_disabled: false, msd_disabled: false}, RebootArch::Arm); // Exiting so that we don't need to hit the boot sel button
                        } else { 
                            let _ = cx.local.led.set_low(); 
                        }
                    }
                }
            }

            let now = cx.local.timer.get_counter();

            if now - last_scan > scan_interval {
                let _ = cx.local.serial.write(b"\r\n--- Starting I2C Bus Scan ---\r\n");
                
                let mut found_any = false;

                // 7-bit addresses are 0x08 through 0x77
                for addr in 0x08u8..0x78u8 {
                    // We attempt a simple 0-byte write to see if the device ACKs
                    // Some HALs require at least 1 byte, so we'll try reading a dummy byte
                    let mut dummy = [0u8; 1];
                    if cx.local.i2c.read(addr, &mut dummy).is_ok() {
                        let mut msg = String::<32>::new();
                        let _ = write!(msg, "Found device at: 0x{:02X}\r\n", addr);
                        let _ = cx.local.serial.write(msg.as_bytes());
                        found_any = true;
                    }
                }

                if !found_any {
                    let _ = cx.local.serial.write(b"No I2C devices found.\r\n");
                }

                let _ = cx.local.serial.write(b"--- Scan Complete ---\r\n");
                last_scan = now;
            }
        }
    }
        
}



