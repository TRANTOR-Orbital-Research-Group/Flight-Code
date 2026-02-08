/* 
 * 
 * Attempting to get the BME280 working with the pico.
 * 
 */


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
    use bme280::i2c::BME280;
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

        // I apologize profusely because of how horrible this looks
        // We should be able to make a type out of this later for readability though
        bme: BME280<
            rp235x_hal::I2C<
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
        >,

    }


    // This is the init task, which is a lot like the `void setup()` function in Arduino cpp
    // Note that it creates the Shared and Local structs that our tasks get to use
    #[init(local = [usb_bus: Option<UsbBusAllocator<hal::usb::UsbBus>> = None])]
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
        let led = pins.gpio25.into_push_pull_output();

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

        // -----------------------------------Added Stuff-----------------------------------

        // Creating a new i2c bus on pins 18 and 19
        let i2c = I2C::i2c1(
                cx.device.I2C1,
                pins.gpio18.reconfigure(), // sda
                pins.gpio19.reconfigure(), // scl
                400.kHz(),
                &mut resets,
                125_000_000.Hz(),
        );

        // Creating the BME
        let mut bme = BME280::new_secondary(i2c);
        bme.init(&mut timer).unwrap();

        // ---------------------------------------------------------------------------------
        
        // Returning our two structs
        (Shared {}, Local { led, timer, usb_dev, serial, bme })
    }



    // This is the idle loop. The idle loop is the basically the `void loop()` part of C++ Arduino
    // The thing above it is a flag that tells Rust what it will have in scope; currently we just have 
    // a local set of variables because we don't need any shared variables right now
    // It takes in a context, which is how you access all of the variables in local and shared.
    #[idle(shared = [], local = [ led, timer, usb_dev, serial, bme ])]
    fn idle(cx: idle::Context) -> ! {

        // This is a simple last time timer implementation
        let mut last_send = cx.local.timer.get_counter();

        // The interval that we are waiting on to send a heartbeat
        let interval = fugit::MicrosDurationU64::micros(2_000_000);

        // This is what you can think of as the actual loop. 
        loop { 

            // The current time (get_counter is a lot like millis in cpp)
            let now = cx.local.timer.get_counter();

            // Polling the usb bus so that we can communicate over it 
            cx.local.usb_dev.poll(&mut [cx.local.serial]);

            // Checking to see if enough time has passed to send a heartbeat
            if (now - last_send) >= interval {

                // -----------------------------------Added Stuff-----------------------------------

                // Taking the measurements
                let measurements = cx.local.bme.measure(cx.local.timer).unwrap();

                // Creating the message string (make sure this isn't too small, if you do it just straight up panics)
                let mut message: String<128> = String::new();
                write!(message, "Humidity: {}%\n\rTemperature: {} deg C\n\rPressure: {} pascals\n\r", measurements.humidity, measurements.temperature, measurements.pressure).unwrap();

                // Writing it 
                let _ = cx.local.serial.write(message.as_bytes());
                

                // -----------------------------------End Added Stuff-----------------------------------

                // let _ = cx.local.serial.write(b"Connected and looping\r\n");
                last_send = now;
            }

            // Putting the CPU to sleed until the next interrupt (Good practice, but we won't be doing it right now)
            // cortex_m::asm::wfi(); 
        }
    }

}