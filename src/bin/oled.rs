#![no_std]
#![no_main]


use esp_alloc as _;
use esp_backtrace as _;
use esp_rtos::{main};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{
    Async, aes::Aes, clock::CpuClock, gpio::{Event, Input, InputConfig, Io, Level, Output, OutputConfig, Pull}, i2c::master::{Config as I2C_Config, I2c}, interrupt::software::SoftwareInterruptControl, ram, rmt::{Channel, PulseCode, Rmt, Tx, TxChannelConfig, TxChannelCreator}, rng::Rng, time::Rate, timer::timg::TimerGroup,
};
use defmt::{ error, info, warn};
use defmt_rtt as _;

use ssd1306_i2c::{prelude::*, Builder}; 

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, ascii::FONT_6X13_BOLD, MonoTextStyleBuilder},
    image::{Image, ImageRawLE},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use esp_println::println;
esp_bootloader_esp_idf::esp_app_desc!();

#[main]
async fn main(_spawner: Spawner) -> ! 
{
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let i2c = I2c::new(peripherals.I2C0, I2C_Config::default().with_frequency(Rate::from_khz(100))).unwrap()
    .with_scl(peripherals.GPIO2)
    .with_sda(peripherals.GPIO3)
    
    ;

    let mut led = Output::new(peripherals.GPIO21, Level::Low, OutputConfig::default());
    let mut display: GraphicsMode<_> = Builder::new()
    .with_size(DisplaySize::Display128x64NoOffset)
    .with_i2c_addr(0x3c)  //or 0x3c
    .with_rotation(DisplayRotation::Rotate0)
    .connect_i2c(i2c)
    .into();

    Timer::after(Duration::from_millis(100)).await;

    if let Err(e) = display.init(){
        led.toggle();
        println!("{:?}",e);
        Timer::after(Duration::from_secs(10)).await;
    };

    display.flush().unwrap();
    
    display.clear();

//   ********* display some text

//   creating MonoTextStyleBuilder
  let text_style = MonoTextStyleBuilder::new()
    .font(&FONT_6X10)
    .text_color(BinaryColor::On)
    .build();

  let text_style_bold = MonoTextStyleBuilder::new()
    .font(&FONT_6X13_BOLD)
    .text_color(BinaryColor::On)
    .build();


    let mut rmt = Rmt::new(peripherals.RMT, Rate::from_mhz(80))
        .expect("Failed to initialize RMT0")
        .into_async()
        .channel0
        .configure_tx(&TxChannelConfig::default().with_clk_divider(8))
        .unwrap()
        .with_pin(peripherals.GPIO0)
        ;

    

  
 

  loop {
    // let ss = heapless::String("",3u8).unwrap();
    Text::with_baseline("----00----", Point::zero(), text_style_bold, Baseline::Top)
    .draw(&mut display)
    .unwrap();
    display.flush().unwrap();
    let _ = rgb_task(&mut rmt).await;
    led.toggle();
    Timer::after(Duration::from_millis(500)).await;
    display.clear();
    display.flush().unwrap();

    led.toggle();
    Timer::after(Duration::from_millis(500)).await;
  }

}

async fn rgb_task(
    rmt:&mut Channel<'_,Async,Tx>,
)
{
    let mut pulses = [PulseCode::default();25];
    // loop {
        
        for i in (0..255u8).step_by(16) {

            rgb(i,i, i,&mut pulses);
            if let Err(e) = rmt.transmit(&pulses).await{
                info!("{:#?}",e);
            }
            Timer::after_millis(100).await;
        }
        for i in (0..255u8).rev().step_by(16) {

            rgb(i,i, i,&mut pulses);

            if let Err(e) = rmt.transmit(&pulses).await{
                info!("{:#?}",e);
            }
            Timer::after_millis(100).await;
        }
    // }
    // channel
}
fn rgb(mut r:u8,mut g:u8,mut b:u8,data:&mut [PulseCode;25]){

    for i in 0..8usize{
        data[i] = if r & 0b10000000u8 == 0b10000000u8{PulseCode::new(Level::High, 8,Level::Low,4)} else {PulseCode::new(Level::High, 4,Level::Low,8)} ;
        r <<= 1; 
    }

    for i in 0..8usize{
        data[8+i] = if g & 0b10000000u8 == 0b10000000u8{PulseCode::new(Level::High, 8,Level::Low,4)} else {PulseCode::new(Level::High, 4,Level::Low,8)} ;
        g <<= 1; 
    }

    for i in 0..8usize{
        data[16+i] = if b & 0b10000000u8 == 0b10000000u8{PulseCode::new(Level::High, 8,Level::Low,4)} else {PulseCode::new(Level::High, 4,Level::Low,8)} ;
        b <<= 1; 
    }
}