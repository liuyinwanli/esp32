#![no_std]
#![no_main]


use esp_alloc as _;
use esp_backtrace as _;
use esp_rtos::{main};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{

    aes::Aes, 
    clock::CpuClock, interrupt::software::SoftwareInterruptControl,
    ram, 
    rng::Rng,
    timer::timg::TimerGroup,
    gpio::{Event, Input, InputConfig, Io, Level, Output, OutputConfig, Pull},
};
use defmt::{ error, info, warn};
use defmt_rtt as _;
esp_bootloader_esp_idf::esp_app_desc!();

#[main]
async fn main(_spawner: Spawner) -> ! 
{
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let mut led = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());

    loop {
        led.toggle();
        info!("翻转");
        Timer::after(Duration::from_millis(500)).await;
    }
}