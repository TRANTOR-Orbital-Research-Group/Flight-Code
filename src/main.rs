/* 
 * 
 * Attempting to get the M9N working with the pico.
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


// -------------------------Added stuff-------------------------
use ublox::{Parser, UbxPacketMeta, FixedLinearBuffer, UbxPacket}; 
use embedded_hal::i2c::I2c;
use hal::prelude::*;
use ublox::proto23::PacketRef; // For the m9n
// use ublox::proto14::PacketRef; // For the m8q
// use ublox::UbxPacketMeta; // Debug
use core::convert::TryFrom;
// use ublox::nav_pvt::proto14::NavPvtRef;
use ublox::nav_pvt::proto23::NavPvtRef;

// Here are the constants for the GPS, we will likely be creating a GPS struct to hold these
const M9N_ADDR: u8 = 0x42;



// -------------------------End added stuff-------------------------


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
    use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
    use rp235x_hal::{pac::{I2C0, otp_data::key1_3}, timer::CopyableTimer0};
    use usb_device::{class_prelude::*, prelude::*};
    use usbd_serial::SerialPort;
    use ublox::UbxPacketMeta;
    // use ublox::nav_pvt::proto14::NavPvtRef;
    use ublox::nav_pvt::proto23::NavPvtRef;
    use core::convert::TryFrom;

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

        // -------------------------Added stuff-------------------------

        // Note the static lifetime; FixedLinearBuffer doesn't own the buffer, just a reference,
        // so if the reference goes out of scope, then everything breaks. Rust gets around that with lifetimes
        // I didn't want to deal with that right now, so I declared it static (so that it never goes out of scope.)
        parser: Parser<FixedLinearBuffer<'static>>,

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
    #[init(local = [usb_bus: Option<UsbBusAllocator<hal::usb::UsbBus>> = None, gps_raw_buffer: [u8; 1024] = [0u8; 1024]])]
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

        // -----------------------------------Added Stuff-----------------------------------

        // Creating a new i2c bus on pins 18 and 19
        let mut i2c = I2C::i2c1(
                cx.device.I2C1,
                pins.gpio18.reconfigure(),  // sda
                pins.gpio19.reconfigure(),  // scl
                100.kHz(),                  // The M8Q I was testing with requires the I2C frequency to be max 100kHz, but the M9N is fine at 400
                &mut resets,
                clocks.peripheral_clock.freq(),
        );


        
        // Telling the GPS that we want the navigation packets
        let enable_nav_pvt = [
            0xB5, 0x62, 0x06, 0x01, 0x03, 0x00, 
            0x01, 0x07, 0x01,
            0x13, 0x51        
        ];
        let _ = i2c.write(M9N_ADDR, &enable_nav_pvt);

        // Telling the GPS to enable the power to the antenna (technically shouldn't be needed but I was having issues)  
        let enable_ant = [0xB5, 0x62, 0x06, 0x13, 0x04, 0x00, 0x1F, 0x00, 0xF0, 0x7D, 0xAB, 0x1F];
        let _ = i2c.write(M9N_ADDR, &enable_ant);

        // The parser that reads the data from I2C and interprets it as UBLOX packets
        let parser = Parser::new(FixedLinearBuffer::new(cx.local.gps_raw_buffer));
                
        // ---------------------------------------------------------------------------------
        
        // Returning our two structs
        (Shared {}, Local { led, timer, usb_dev, serial, parser, i2c })
    }



    // This is the idle loop. The idle loop is the basically the `void loop()` part of C++ Arduino
    // The thing above it is a flag that tells Rust what it will have in scope; currently we just have 
    // a local set of variables because we don't need any shared variables right now
    // It takes in a context, which is how you access all of the variables in local and shared.
    #[idle(shared = [], local = [ led, timer, usb_dev, serial, parser, i2c ])]
    fn idle(cx: idle::Context) -> ! {

        // This is a simple last time timer implementation
        let mut last_send = cx.local.timer.get_counter();

        // The interval that we are waiting on to send a heartbeat
        let interval = fugit::MicrosDurationU64::micros(1_000_000);
        // fugit::Duration::secs(2);

        // Just a counter that I wanted to only sometimes poll for the hardware condition
        // it polls every 5 loops
        let mut hw_poll_counter = 0;

        loop {

            // Polling for any response by the computer over Serial
            if cx.local.usb_dev.poll(&mut [cx.local.serial]) {
                let mut buf = [0u8; 64];
                if let Ok(count) = cx.local.serial.read(&mut buf) {
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

            // Same as before to set up non-blocking
            let now = cx.local.timer.get_counter();


            // -----------------------------------Added Stuff-----------------------------------

            if now - last_send > interval {

                // The two bytes that represent the length (we will send it as a buffer so that I2C can tell us how much data is being sent) 
                let mut length_bytes = [0u8; 2];

                // Telling the GPS that we want data and asking it how much it is ready to send
                if cx.local.i2c.write_read(M9N_ADDR, &[0xFD], &mut length_bytes).is_ok() {

                    // How much data it is ready to send
                    let bytes_available = u16::from_be_bytes(length_bytes) as usize;

                    // A running total of how many bytes we have left to read so we can iterate across them
                    let mut total_to_read = bytes_available;

                    // Iterating over all of the bytes
                    while total_to_read > 0 {
                        
                        // Making sure that the maximum amount of data we get will still fit in our buffer
                        let mut data_chunk = [0u8; 64]; 
                        let to_read = core::cmp::min(total_to_read, data_chunk.len());
                                
                        // Asking for the information from the gps
                        if cx.local.i2c.write_read(M9N_ADDR, &[0xFF], &mut data_chunk[..to_read]).is_ok() {

                            // Passing the stuff to the parser
                            let mut it = cx.local.parser.consume_ubx(&data_chunk[..to_read]);

                            // Iterating through the data that the parser has to read the packets
                            while let Some(packet_result) = it.next() {

                                // If we have a packet, we need to read it
                                if let Ok(packet) = packet_result {

                                    // Matching the protocol of the packet (this is technically not needed, but I was testing with a GPS that
                                    // uses a different protocol than the M9N, so this might help prevent headaches later
                                    if let UbxPacket::Proto23(p) = packet {
                                        
                                        // Now that we know that we have the packet, we need to know which kind
                                        match p {

                                                // The NavPvt packet is the navagation packet we want
                                                PacketRef::NavPvt(nav_pvt) => {
                                                    let mut s = String::<128>::new();
                                                    let _ = write!(s, "Fix: {:?} | Sats: {} | Local: ({},{}) | Acc: {}\r\n", 
                                                        nav_pvt.fix_type(), 
                                                        nav_pvt.num_satellites(),
                                                        nav_pvt.longitude(),
                                                        nav_pvt.latitude(),
                                                        nav_pvt.horizontal_accuracy() // If this is 4294967295, the antenna is likely disconnected/unpowered
                                                    );
                                                    let _ = cx.local.serial.write(s.as_bytes());
                                                    s = String::<128>::new();
                                                    let _ = write!(s, "{} {}, {} {}:{}:{}\r\n", 
                                                        nav_pvt.year(), 
                                                        nav_pvt.month(),
                                                        nav_pvt.day(),
                                                        nav_pvt.hour(),
                                                        nav_pvt.min(),
                                                        nav_pvt.sec(),
                                                    );
                                                    let _ = cx.local.serial.write(s.as_bytes());
                                                },
                                                // This is a packet that says the status of the GPS, so like the status and any errors
                                                PacketRef::NavStatus(status) => {
                                                    let mut s = String::<64>::new();
                                                    let _ = write!(s, "NavStatus: {:?} | Flags: 0x{:02X}\r\n", 
                                                        status.itow(), 
                                                        status.flags()
                                                    );
                                                    let _ = cx.local.serial.write(s.as_bytes());
                                                },
                                                // If there are any unknown packets, this is where we can catch them
                                                // in my testing I didn't actually ever get one though
                                                PacketRef::Unknown(_) =>  {
                                                    let mut s = String::<128>::new();
                                                    // Use standard formatting for protocol 14
                                                    let _ = write!(s, "We got an unknown packet type)\r\n");
                                                    let _ = cx.local.serial.write(s.as_bytes());
                                                },
                                                // This is the hardware status packet, you have to specifically request it 
                                                // but you can use the noise to tell you how close you are to getting a lock
                                                // Noise below 90 will quickly get a lock, slowly between 90 and 100, and rarely above 100
                                                PacketRef::MonHw(hw) => {
                                                    let mut s = String::<128>::new();
                                                    let _ = write!(s, "Noise: {} | AGC: {}% | AntStatus: {:?}\r\n", 
                                                        hw.noise_per_ms(), 
                                                        (hw.agc_cnt() as u32 * 100) / 8191, 
                                                        hw.a_status()
                                                    );
                                                    let _ = cx.local.serial.write(s.as_bytes());
                                                },
                                                // The wildcard, so any other packet that we don't have accounted for 
                                                _ => {
                                                    // Printing out what the ID and class of the packet is 
                                                    let (class, id) = p.class_and_msg_id();
                                                    let mut debug_msg = String::<64>::new();
                                                    let _ = write!(debug_msg, "Unimplemented packet recieved: Class 0x{:02X}, ID 0x{:02X}\r\n", class, id);
                                                    let _ = cx.local.serial.write(debug_msg.as_bytes());
                                                },
                                        };
                                    }
                                }
                            }
                        }
                        total_to_read -= to_read;
                    }
                } else {
                    let _ = cx.local.serial.write(b"Didn't get a message back on the i2c request\r\n");
                    let mut debug_msg = String::<64>::new();
                    let _ = write!(debug_msg, "Still has {} bytes\r\n", cx.local.parser.buffer_len());
                    let _ = cx.local.serial.write(debug_msg.as_bytes());
                }

                // This is the logic for requesting a hardware info packet
                hw_poll_counter += 1;
                if hw_poll_counter >= 5 {
                    let mon_hw_poll = [0xB5, 0x62, 0x0A, 0x09, 0x00, 0x00, 0x13, 0x43];
                    let _ = cx.local.i2c.write(M9N_ADDR, &mon_hw_poll);
                    hw_poll_counter = 0;
                }

                last_send = now;

            }

            cx.local.timer.delay_ms(10);

            // -----------------------------------End Added Stuff-----------------------------------

            // Putting the CPU to sleep until the next interrupt (Good practice, but we won't be doing it right now)
            // cortex_m::asm::wfi(); 

        }
    }

}




