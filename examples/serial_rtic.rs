#![no_std]
#![no_main]

use rp235x_hal as hal;
use {panic_probe as _};
use embedded_hal::digital::OutputPin;
use defmt_rtt as _;
use rp235x_hal::reboot::{reboot, RebootKind, RebootArch};

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
        let timer = hal::Timer::new_timer0(cx.device.TIMER0, &mut resets, &clocks);

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
        
        // Returning our two structs
        (Shared {}, Local { led, timer, usb_dev, serial })
    }



    // This is the idle loop. The idle loop is the basically the `void loop()` part of C++ Arduino
    // The thing above it is a flag that tells Rust what it will have in scope; currently we just have 
    // a local set of variables because we don't need any shared variables right now
    // It takes in a context, which is how you access all of the variables in local and shared.
    #[idle(shared = [], local = [timer, serial, led, usb_dev])]
    fn idle(cx: idle::Context) -> ! {

        // This is a simple last time timer implementation
        let mut last_send = cx.local.timer.get_counter();

        // The interval that we are waiting on to send a heartbeat
        let interval = fugit::MicrosDurationU64::micros(2_000_000);

        // This is what you can think of as the actual loop. 
        loop {

           // Polling the usb device to see if we have anything extra to play with from the other device
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

            // The current time (get_counter is a lot like millis in cpp)
            let now = cx.local.timer.get_counter();

            // Checking to see if enough time has passed to send a heartbeat
            if (now - last_send) >= interval {
                let _ = cx.local.serial.write(b"I like waffle fries!\r\n");
                last_send = now;
            }

            // Putting the CPU to sleed until the next interrupt (Good practice, but we won't be doing it right now)
            // cortex_m::asm::wfi(); 
        }
    }

}