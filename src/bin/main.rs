#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use esp_hal::{
    gpio::{Level},
    clock::{self,CpuClock},
    timer::timg::TimerGroup,

    rmt::{Tx,Rmt,TxChannelConfig,PulseCode,TxChannelCreator,Channel}, time::Rate,
    Async,

    efuse::{self, InterfaceMacAddress}
};

use esp_radio::ble::controller::BleConnector;
use trouble_host::prelude::*;
use embassy_futures::{join::join, select::select3};

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
// use bt_hci::controller::ExternalController;
use defmt::{info,warn,error};
use defmt_rtt as _;
// use esp_backtrace as _;
#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}



const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 1;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) {

   
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));


    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO4
    // - GPIO5
    // - GPIO8
    // - GPIO9
    // - GPIO15
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO24;
    let _ = peripherals.GPIO25;
    let _ = peripherals.GPIO26;
    let _ = peripherals.GPIO27;
    let _ = peripherals.GPIO28;
    let _ = peripherals.GPIO29;
    let _ = peripherals.GPIO30;
    info!("========================================");
    info!("             ESP32 硬件信息报告           ");
    info!("========================================");

    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);

    info!("MAC 地址 bytes: {:02x}", mac.as_bytes());

    let rev = efuse::chip_revision();
    info!("芯片硅片版本 (Revision): v{}.{}",  rev.major, rev.minor);

    info!("CPU 主频: {} MHz", clock::cpu_clock().as_mhz());

    info!("========================================");

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");

    // find more examples https://github.com/embassy-rs/trouble/tree/main/examples/esp32
    let transport = BleConnector::new(peripherals.BT, Default::default()).unwrap();
    let ble_controller = ExternalController::<_, 1>::new(transport);

    let _ = spawner;

    let frequency = Rate::from_mhz(80);
    // let rmt = ;
    let mut channel = Rmt::new(peripherals.RMT, frequency)
        .expect("Failed to initialize RMT0")
        .into_async()
        .channel0
        .configure_tx(&TxChannelConfig::default().with_clk_divider(8))
        .unwrap()
        .with_pin(peripherals.GPIO8)
        ;
//
    // ble_bas_periphqeral_run(ble_controller,&mut channel).await;

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}



// GATT Server definition
/// Server
#[gatt_server]
struct Server {
    // battery_service: BatteryService,
    mine: Mine,
}


/// Mine
#[gatt_service(uuid = "123413df-5dd4-1f87-ec11-cdb001100000")]
struct Mine {
    /// Battery Level
    // #[descriptor(uuid = descriptors::VALID_RANGE, read, value = [0, 100])]
    // #[descriptor(uuid = descriptors::MEASUREMENT_DESCRIPTION, name = "hello", read, value = "Battery Level", type = &'static str)]
    #[characteristic(uuid = "234513df-5dd4-1f87-ec11-cdb001100000", read, value = 10)]
    xread: u8,
    #[characteristic(uuid = "345613df-5dd4-1f87-ec11-cdb001100000", notify, value = 10)]
    xnotify: u8,
    #[characteristic(uuid = "408813df-5dd4-1f87-ec11-cdb001100000", write)]
    xwrite: [u8;3],
}
/// Run the BLE stack.
pub async fn ble_bas_peripheral_run<C>(controller: C,channel:&mut Channel<'_,Async,Tx>)
where
    C: Controller ,
{
    // Using a fixed "random" address can be useful for testing. In real scenarios, one would
    // use e.g. the MAC 6 byte array as the address (how to get that varies by the platform).
    let address: Address = Address::random([0xff, 0x8f, 0x1a, 0x05, 0xe4, 0xff]);
    info!("Our address = {:?}", address);

    let mut resources: HostResources<_, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .set_io_capabilities(IoCapabilities::NoInputNoOutput)
        .build()
        
        ;

    let mut peripheral = stack.peripheral();

    info!("Starting advertising and GATT service");
    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "TrouBLEx",
        appearance: &appearance::power_device::GENERIC_POWER_DEVICE,
    }))
    .unwrap();

    let _ = join(ble_task(stack.runner()), async {
        loop {
            match advertise("Troublex", &mut peripheral, &server).await {
                Ok(conn) => {
                    // set up tasks when the connection is established to a central, so they don't
                    // run when no one is connected.
                    let a = gatt_events_task(&server, &conn);
                    let b = custom_task(&server, &conn, &stack,);
                    let c = led_task(&server,channel);
                    // run until any task ends (usually because the connection has been closed),
                    // then return to advertising state.
                    select3(a, b,c).await;
                    
                }
                Err(e) => {
                    panic!("[adv] error: {:?}", e);
                }
                _ => {}
            }
        }
    })
    .await;
}

/// This is a background task that is required to run forever alongside any other BLE tasks.
///
/// ## Alternative
///
/// If you didn't require this to be generic for your application, you could statically spawn this
/// with i.e.
///
/// ```rust,ignore
/// 
/// #[embassy_executor::task]
/// async fn ble_task(mut runner: Runner<'static, SoftdeviceController<'static>>) {
///     runner.run().await;
/// }
///
/// spawner.spawn(ble_task(runner).unwrap());
/// ```
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    loop {
        if let Err(e) = runner.run().await {
            panic!("[ble_task] error: {:?}", e);
        }
    }
}

/// Stream Events until the connection closes.
///
/// This function will handle the GATT events and process them.
/// This is how we interact with read and write requests.
async fn gatt_events_task<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
) -> Result<(), Error> {
    let readx = server.mine.xread;
    let writex = server.mine.xwrite;
    let reason = loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => break reason,
            GattConnectionEvent::Gatt { event } => {
                match &event {
                    GattEvent::Read(event) => {
                        if event.handle() == readx.handle {
                            let value = server.get(&readx);
                            info!("[gatt] Read Event to Level Characteristic: {:?}", value);
                        }
                    }
                    GattEvent::Write(event) => {
                        event.with_data(|offset, data| {
                            if event.handle() == writex.handle && data.len() >= 3{
  
                                info!("数据有效");
                                let mut ret = [0u8;3];
                                ret[0] = data[0];
                                ret[1] = data[1];
                                ret[2] = data[2];
                                server.set(&writex, &ret);
                            }    
                        });
                    }
                    _ => {}
                };
                // This step is also performed at drop(), but writing it explicitly is necessary
                // in order to ensure reply is sent.
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("[gatt] error sending response: {:?}", e),
                };
            }
            _ => {} // ignore other Gatt Connection Events
        }
    };
    info!("[gatt] disconnected: {:?}", reason);
    Ok(())
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut advertiser_data = [0; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::IncompleteServiceUuids16(&[[0x0f, 0x18]]),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut advertiser_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..len],
                scan_data: &[],
            },
        )
        .await?;
    info!("[adv] advertising");
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    info!("[adv] connection established");
    Ok(conn)
}

/// Example task to use the BLE notifier interface.
/// This task will notify the connected central of a counter value every 2 seconds.
/// It will also read the RSSI value every 2 seconds.
/// and will stop when the connection is closed by the central or an error occurs.
async fn custom_task<C: Controller, P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    stack: &Stack<'_, C, P>,
) {
    let mut tick: u8 = 0;
    let notifyx = server.mine.xnotify;
    loop {
        // tick = tick.wrapping_add(1);
        // info!("[custom_task] notifying connection of tick {}", tick);
        if notifyx.notify(conn, server.get(&notifyx).as_ref().unwrap(),true).await.is_err() {
            // info!("[custom_task] error notifying connection");
            break;
        };
        // read RSSI (Received Signal Strength Indicator) of the connection.
        if let Ok(rssi) = conn.raw().rssi(stack).await {
            // info!("[custom_task] RSSI: {:?}", rssi);
        } else {
            info!("[custom_task] error getting RSSI");
            break;
        };
        Timer::after_secs(2).await;
    }
}
async fn led_task(
    server: &Server<'_>,
    channel:&mut Channel<'_,Async,Tx>,
){
    let mut pulses = [PulseCode::default();25];
    loop {
        
        for i in (0..255u8).step_by(16) {

            rgb(i,i, i,&mut pulses);
            if let Err(e) = channel.transmit(&pulses).await{
                info!("{:#?}",e);
            }
            Timer::after_millis(100).await;
        }
        for i in (0..255u8).rev().step_by(16) {

            rgb(i,i, i,&mut pulses);

            if let Err(e) = channel.transmit(&pulses).await{
                info!("{:#?}",e);
            }
            Timer::after_millis(100).await;
        }
    }
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