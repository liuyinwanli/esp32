//! Embassy access point with station
//!
//! Set SSID and PASSWORD env variable before running the example.
//!
//! - gets an ip address via DHCP
//! - creates an open access-point with SSID `esp-radio`
//! - if you either:
//!   - connect to it using a static IP in range 192.168.2.2 .. 192.168.2.255, gateway 192.168.2.1
//!   - open http://192.168.2.1:8080/ in your browser
//! - or:
//!   - connect to the network referenced by the SSID env variable and open the IP address printed
//!     by the example
//! - the example will perform an HTTP get request to some "random" server and return the response
//!
//! On Android you might need to choose _Keep Accesspoint_ when it tells you the
//! WiFi has no internet connection, Chrome might not want to load the URL - you
//! can use a shell and try `curl` and `ping`

//% CHIP_FILTER: wifi_driver_supported

#![no_std]
#![no_main]
use core::
use core::{net::Ipv4Addr, str};

use embassy_executor::Spawner;
use embassy_futures::select::Either;
use embassy_net::{
    IpListenEndpoint,
    Ipv4Cidr,
    Runner,
    StackResources,
    StaticConfigV4,
    dns::DnsSocket,
    tcp::{
        TcpSocket,
        client::{TcpClient, TcpClientState},
    },
    udp::{
        UdpSocket,
        UdpMetadata,
        PacketMetadata,
    }
};
use embassy_time::{Duration, Timer};
use embedded_io_async::Read;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    interrupt::software::SoftwareInterruptControl,
    ram,
    rng::Rng,
    timer::timg::TimerGroup,
};
use defmt::{Str, error, info, warn};
use defmt_rtt as _;
use esp_radio::wifi::{
    Config,
    ControllerConfig,
    Interface,
    WifiController,
    ap::AccessPointConfig,
    sta::StationConfig,
};
use reqwless::{
    client::HttpClient,
    request::{Method, RequestBuilder},
};
esp_bootloader_esp_idf::esp_app_desc!();

const SSID: &str = "ll_2G";
const PASSWORD: &str = "sN56NsLs@ll";

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let access_point_station_config = Config::AccessPointStation(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into()),
        AccessPointConfig::default().with_ssid("esp-radio"),
    );

    info!("Starting wifi");

    let wifi_sta_device = esp_radio::wifi::Interface::station();
    let controller = esp_radio::wifi::WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(access_point_station_config),
    )
    .unwrap();


   
    let sta_config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    
    let (sta_stack, sta_runner) = embassy_net::new(
        wifi_sta_device,
        sta_config,
        mk_static!(StackResources<4>, StackResources::<4>::new()),
        seed,
    );

    spawner.spawn(connection(controller).unwrap());
    // spawner.spawn(net_task(ap_runner).unwrap());
    spawner.spawn(net_task(sta_runner).unwrap());

    let sta_address = loop {
        if let Some(config) = sta_stack.config_v4() {
            let address = config.address.address();
            info!("Got IP: {}", address);
            break address;
        }
        info!("Waiting for IP...");
        Timer::after(Duration::from_millis(500)).await;
    };


    info!("Connect to the ap `{}` and point your browser to http://{}:8080/",SSID,sta_address);

    // Init HTTP client
    let tcp_client = TcpClient::new(
        sta_stack,
        mk_static!(
            TcpClientState<1, 1500, 1500>,
            TcpClientState::<1, 1500, 1500>::new()
        ),
    );

    let mut sta_server_rx_buffer = [0; 1536];
    let mut sta_server_tx_buffer = [0; 1536];


    let mut tcp_socket = TcpSocket::new(
        sta_stack,
        &mut sta_server_rx_buffer,
        &mut sta_server_tx_buffer,
    );
    tcp_socket.set_timeout(Some(embassy_time::Duration::from_secs(10)));
    

    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0u8; 1536];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0u8; 1536];

    let mut udp_socket = UdpSocket::new(sta_stack, &mut rx_meta,&mut rx_buffer,&mut tx_meta,&mut tx_buffer);
    
    udp_socket.bind(IpListenEndpoint {
        addr: None,
        port: 8080,
    });
    
    let dns_client = DnsSocket::new(sta_stack);
    let mut sta_server_socket = tcp_socket;
    let mut buffer = [0u8; 1024];
    loop {
        if let Ok((len,data)) =  udp_socket.recv_from(&mut buffer).await && len <= 1024{
            let str = str::from_utf8(&buffer[..len]).unwrap();
            info!("{} {}",len,str);
        }
    }
    /*
    loop {
        info!("Wait for connection...");
        // FIXME: If connections are attempted on both sockets at the same time, we
        // might end up dropping one of them. Might be better to spawn both
        // accept() calls, or use fused futures? Note that we only attempt to
        // serve one connection at a time, so we don't run out of ram.

        if let Err(_) = sta_server_socket.accept(IpListenEndpoint {
            addr: None,
            port: 8080,
        }).await{
            continue;
        }

        info!("Connected...");


        use embedded_io_async::Write;

        let mut buffer = [0u8; 1024];
        let mut pos = 0;
        loop {
            match sta_server_socket.read(&mut buffer).await {
                Ok(0) => {
                    info!("AP read EOF");
                    break;
                }
                Ok(len) => {
                    let to_print =
                        unsafe { core::str::from_utf8_unchecked(&buffer[..(pos + len)]) };

                    if to_print.contains("\r\n\r\n") {

                        info!("{}", to_print);
                        break;
                    }

                    pos += len;
                }
                Err(e) => {
                    info!("AP read error: {:?}", e);
                    break;
                }
            };
        }
        if sta_stack.is_link_up() {
            let x = udp_socket.bind(IpListenEndpoint {
                addr: None,
                port: 8080,
            }).unwrap()
        } else {
            let r = sta_server_socket
                .write_all(
                    b"HTTP/1.0 200 OK\r\n\r\n\
                    <html>\
                        <body>\
                            <h1>Hello Rust! Hello esp-radio! Station is not connected.</h1>\
                        </body>\
                    </html>\r\n\
                    ",
                )
                .await;
            if let Err(e) = r {
                info!("AP write error: {:?}", e);
            }
        }
        let r = sta_server_socket.flush().await;
        if let Err(e) = r {
            info!("AP flush error: {:?}", e);
        }
        Timer::after(Duration::from_millis(1000)).await;
        sta_server_socket.close();
        Timer::after(Duration::from_millis(1000)).await;
        sta_server_socket.abort();
    }
    */
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    info!("start connection task");

    loop {
        match controller.connect_async().await {
            Ok(_) => {
                // wait until we're no longer connected
                loop {
                    let info = embassy_futures::select::select(
                        controller.wait_for_disconnect_async(),
                        controller.wait_for_access_point_connected_event_async(),
                    )
                    .await;

                    match info {
                        Either::First(station_disconnected) => {
                            if let Ok(station_disconnected) = station_disconnected {
                                info!("Station disconnected: {:?}", station_disconnected);
                                break;
                            }
                        }
                        Either::Second(event) => {
                            if let Ok(event) = event {
                                match event {
                                    esp_radio::wifi::ap::EventInfo::Connected(info) => {
                                        info!("Station connected: {:?}", info);
                                    }
                                    esp_radio::wifi::ap::EventInfo::Disconnected(info) => {
                                        info!("Station disconnected: {:?}", info);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to wifi: {}",e);
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}
