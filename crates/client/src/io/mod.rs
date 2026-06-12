// jagex3.io package

pub mod buffered_random_access_file;
pub mod byte_array_node;
pub mod byte_array_pool;
pub mod cache_util;
pub mod file_on_disk;
pub mod isaac;
pub mod byte_array_wrapper;
pub mod packet;
pub mod gzip;
pub mod bzip2;
pub mod client_stream;
pub mod data_file;
pub mod ws_socket;

// The socket type sign_link's socketreq produces and ClientStream::new
// consumes — TCP natively, a browser WebSocket on wasm.
#[cfg(not(target_arch = "wasm32"))]
pub type NetSocket = std::net::TcpStream;
#[cfg(target_arch = "wasm32")]
pub type NetSocket = ws_socket::WsSocket;
