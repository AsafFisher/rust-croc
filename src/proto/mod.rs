pub mod client_session;
mod croc_enc;
mod croc_msg;
mod croc_raw;
pub use croc_enc::EncryptedSession;
pub use croc_raw::CrocProto;

#[cfg(test)]
mod test {
    use std::net::TcpStream;

    use crypto::pake::{Pake, Role};

    #[test]
    fn test_ping_proto() {
        // let default_relay_addr = "croc.schollz.com:9009";
        // {
        //     let _pake = Pake::new(Role::Sender);

        //     let mut proto = CrocProto::connect(default_relay_addr).unwrap();
        //     proto.write(b"ping").unwrap();
        //     assert_eq!(String::from_utf8_lossy(&proto.read().unwrap()), "pong")
        // }
        // {
        //     let _pake = Pake::new(Role::Sender);
        //     let stream = TcpStream::connect(default_relay_addr).unwrap();
        //     let mut proto = CrocProto::from_stream(stream);
        //     proto.write(b"ping").unwrap();
        //     assert_eq!(String::from_utf8_lossy(&proto.read().unwrap()), "pong")
        // }
    }

    #[test]
    fn test_process_message() {
        let _hello =
            process_message("{\"t\": \"pake\", \"b\": \"hello\", \"b2\":\"hello\"}".as_bytes())
                .unwrap();
    }
}
