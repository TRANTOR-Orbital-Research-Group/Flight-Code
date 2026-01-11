
#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_time::Timer;
use gpio::{Level, Output};
use {defmt_rtt as _, panic_probe as _};

// Program metadata for `picotool info`.
// This isn't needed, but it's recomended to have these minimal entries.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"Blinky Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"This example tests the RP Pico on board LED, connected to gpio 25"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut led = Output::new(p.PIN_25, Level::Low);

    info!("Hello, World!");

    let sdcard = SdCard::new(sdmmc_spi, delay);
    let volume_mgr = VolumeManager::new(sdcard, time_source);
    // Try and access Volume 0 (i.e. the first partition).
    // The volume object holds information about the filesystem on that volume.
    let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
    // println!("Volume 0: {:?}", volume0);
    // Open the root directory (mutably borrows from the volume).
    let root_dir = volume0.open_root_dir()?;


    let my_other_file = root_dir.open_file_in_dir("MY_DATA.CSV", embedded_sdmmc::Mode::ReadWriteCreateOrAppend)?;
    my_other_file.write(b"Timestamp,Signal,Value\n")?;
    my_other_file.write(b"2025-01-01T00:00:00Z,TEMP,25.0\n")?;
    my_other_file.write(b"2025-01-01T00:00:01Z,TEMP,25.1\n")?;
    my_other_file.write(b"2025-01-01T00:00:02Z,TEMP,25.2\n")?;

    // Don't forget to flush the file so that the directory entry is updated
    my_other_file.flush()?;


    loop {
        led.set_high();
        Timer::after_millis(250).await;

        led.set_low();
        Timer::after_millis(250).await;
    }
}

// Example code I copied over to mess with
use embedded_sdmmc::{Error, Mode, SdCard, SdCardError, TimeSource, VolumeIdx, VolumeManager};

fn example<S, D, T>(spi: S, delay: D, ts: T) -> Result<(), Error<SdCardError>>
where
    S: embedded_hal::spi::SpiDevice,
    D: embedded_hal::delay::DelayNs,
    T: TimeSource,
{
    let sdcard = SdCard::new(spi, delay);
    // println!("Card size is {} bytes", sdcard.num_bytes()?);
    let volume_mgr = VolumeManager::new(sdcard, ts);
    let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
    // println!("Volume 0: {:?}", volume0);
    let root_dir = volume0.open_root_dir()?;
    let mut my_file = root_dir.open_file_in_dir("testing.TXT", Mode::ReadOnly)?;
    while !my_file.is_eof() {
        let mut buffer = [0u8; 32];
        let num_read = my_file.read(&mut buffer)?;
        for b in &buffer[0..num_read] {
            // print!("{}", *b as char);
        }
    }
    Ok(())
}

use embedded_sdmmc::{BlockDevice, Directory};
fn write_file<D: BlockDevice, T: TimeSource, const DIRS: usize, const FILES: usize, const VOLUMES: usize>(
    root_dir: &mut Directory<D, T, DIRS, FILES, VOLUMES>,
) -> Result<(), Error<D::Error>>
{
    let my_other_file = root_dir.open_file_in_dir("MY_DATA.CSV", Mode::ReadWriteCreateOrAppend)?;
    my_other_file.write(b"Timestamp,Signal,Value\n")?;
    my_other_file.write(b"2025-01-01T00:00:00Z,TEMP,25.0\n")?;
    my_other_file.write(b"2025-01-01T00:00:01Z,TEMP,25.1\n")?;
    my_other_file.write(b"2025-01-01T00:00:02Z,TEMP,25.2\n")?;
    // Don't forget to flush the file so that the directory entry is updated
    my_other_file.flush()?;
    Ok(())
}