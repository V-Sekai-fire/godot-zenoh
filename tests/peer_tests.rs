
#[derive(Debug)]
struct TestPeer {
    current_channel: i32,
}

impl TestPeer {
    fn new() -> Self {
        TestPeer {
            current_channel: 0,
        }
    }

    fn set_transfer_channel(&mut self, channel: i32) {
        self.current_channel = channel;
    }

    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    fn get_packet(&self, _buffer: &mut [u8]) -> Result<(), &'static str> {
        Err("No packets available - local queuing disabled")
    }

    fn get_available_packet_count(&self) -> i32 {
        0
    }
}

#[test]
fn test_channel_setting() {
    let mut peer = TestPeer::new();
    peer.set_transfer_channel(42);
    assert_eq!(peer.transfer_channel(), 42);
    peer.set_transfer_channel(255);
    assert_eq!(peer.transfer_channel(), 255);
}

#[test]
fn test_no_packets() {
    let peer = TestPeer::new();
    let mut buffer = vec![0u8; 10];
    let result = peer.get_packet(buffer.as_mut_slice());
    assert!(result.is_err());
    assert_eq!(peer.get_available_packet_count(), 0);
}
