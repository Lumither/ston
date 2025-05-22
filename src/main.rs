use std::io;

use clap::Parser;
use log::{debug, error, info};
use tokio::net::{TcpListener, TcpStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{broadcast, mpsc},
};
use tokio_serial::SerialPortBuilderExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A simple serial tty to network bridge.
/// This allows you to access serial devices via network connections, like telnet.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// path to the serial device (eg. /dev/ttyUSB0)
    #[arg(short, long)]
    device: String,

    /// baud rate
    #[arg(short, long, default_value_t = 115200)]
    baud: u32,

    /// <IP:PORT> listening
    #[arg(short, long, default_value = "0.0.0.0:7070")]
    connection: String,
}

const MAX_BUFFER_SIZE: usize = 1024;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .format_target(false)
        .init();

    let args = Args::parse();

    let device = args.device;
    let baud = args.baud;
    let connection = args.connection;

    let (tx_net, rx_net) = mpsc::channel::<Vec<u8>>(MAX_BUFFER_SIZE);
    let (tx_ser, _rx_ser) = broadcast::channel::<Vec<u8>>(MAX_BUFFER_SIZE);

    let serial_conn = tokio::spawn({
        let tx_ser = tx_ser.clone();
        async move { serial_handler(&device, baud, rx_net, tx_ser).await }
    });

    let net_conn = tokio::spawn(async move { net_handler(&connection, tx_ser, tx_net).await });

    tokio::select! {
        _ = serial_conn => {
            info!("Serial task finished, exiting");
        }
        _ = net_conn => {
            info!("Network task finished, exiting");
        }
    }

    Ok(())
}

async fn serial_handler(
    device_path: &str,
    baud: u32,
    mut rx_net: mpsc::Receiver<Vec<u8>>,
    tx_ser: broadcast::Sender<Vec<u8>>,
) -> Result<()> {
    let mut serial_device = tokio_serial::new(device_path, baud).open_native_async()?;
    info!("Serial device connected: {}@{}", device_path, baud);

    let mut serial_read_buf = [0u8; MAX_BUFFER_SIZE];

    loop {
        tokio::select! {
            read_res = serial_device.read(&mut serial_read_buf) => {
                match read_res {
                    Ok(bytes_read) => {
                        debug!("Received {} bytes from network", bytes_read);
                        if bytes_read > 0 {
                            if let Err(e) = tx_ser.send(serial_read_buf[..bytes_read].to_vec()) {
                                error!("Failed to send data to network: {}", e);
                            }
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::TimedOut=> {}
                    Err(e) => {
                        error!("Failed to read serial device: {}", e);
                        return Err(Box::new(e));
                    }
                }
            },
            net_data = rx_net.recv() => {
                if let Some(data) = net_data {
                    debug!("Received {} bytes from network", data.len());
                    if let Err(e) = serial_device.write_all(&data).await {
                        error!("Failed to write to serial device: {}", e);
                        return Err(Box::new(e));
                    }
                } else {
                    info!("All clients disconnected, closing serial connection");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn net_handler(
    connection: &str,
    tx_ser: broadcast::Sender<Vec<u8>>,
    tx_net: mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    let listener = TcpListener::bind(connection).await?;
    info!("Listening on {}", connection);
    println!("Listening on {}", connection);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        info!("Accepted connection from {}", peer_addr);

        let client_tx = tx_net.clone();
        let client_rx = tx_ser.subscribe();

        tokio::spawn(async move {
            if let Err(e) = handle_tcp_client(stream, client_rx, client_tx).await {
                error!("Failed to handle TCP client: {}", e);
            }
            info!("Client disconnected: {}", peer_addr);
        });
    }
}

async fn handle_tcp_client(
    mut stream: TcpStream,
    mut ser_to_net: broadcast::Receiver<Vec<u8>>,
    net_to_ser: mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    let (mut reader, mut writer) = stream.split();

    let mut read_buf = [0u8; MAX_BUFFER_SIZE];

    loop {
        tokio::select! {
            read_res = reader.read(&mut read_buf) => {
                match read_res {
                    Ok(0) => {
                        debug!("Client disconnected");
                        break;
                    },
                    Ok(bytes_read) => {
                        debug!("Received {} bytes from TCP client", bytes_read);
                        if let Err(e) = net_to_ser.send(read_buf[..bytes_read].to_vec()).await {
                            error!("Failed to send data to serial device: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        error!("Failed to read TCP client: {}", e);
                        return Err(Box::new(e));
                    }
                }
            },
            serial_data_res = ser_to_net.recv() => {
                match serial_data_res {
                    Ok(data) => {
                        debug!("Received {} bytes from serial device", data.len());
                        if let Err(e) = writer.write_all(&data).await {
                            error!("Failed to write TCP client: {}", e);
                            return Err(Box::new(e));
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        error!("Lost {} bytes from serial device", n);
                    },
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Serial device disconnected, closing TCP connection");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
