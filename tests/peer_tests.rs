use rstest::rstest;
use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};

/// Simplified test peer structure for unit testing channel logic
#[derive(Debug)]
struct TestPeer {
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Vec<u8>>>>>,
    current_channel: i32,
}

impl TestPeer {
    fn new() -> Self {
        TestPeer {
            packet_queues: Arc::new(Mutex::new(HashMap::new())),
            current_channel: 0,
        }
    }

    fn set_transfer_channel(&mut self, channel: i32) {
        self.current_channel = channel;
    }

    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    fn add_packet_to_channel(&self, channel: i32, data: Vec<u8>) {
        let mut queues = self.packet_queues.lock().unwrap();
        queues.entry(channel).or_insert_with(VecDeque::new).push_back(data);
    }

    fn get_packet(&self, buffer: &mut [u8]) -> Result<(), &'static str> {
        let mut queues = self.packet_queues.lock().unwrap();

        // Find lowest channel number with packets
        for channel in 0..=255 {
            if let Some(queue) = queues.get_mut(&channel) {
                if let Some(packet) = queue.pop_front() {
                    let len = std::cmp::min(packet.len(), buffer.len());
                    buffer[..len].copy_from_slice(&packet[..len]);
                    return Ok(());
                }
            }
        }

        Err("No packets available")
    }

    fn get_available_packet_count(&self) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        queues.values().map(|q| q.len() as i32).sum()
    }
}

#[cfg(test)]
mod peer_channel_tests {
    use super::*;

    #[test]
    fn test_channel_setting() {
        let mut peer = TestPeer::new();

        assert_eq!(peer.transfer_channel(), 0);
        peer.set_transfer_channel(42);
        assert_eq!(peer.transfer_channel(), 42);
        peer.set_transfer_channel(255);
        assert_eq!(peer.transfer_channel(), 255);
    }

    #[test]
    fn test_channel_isolation() {
        let peer = TestPeer::new();

        // Add packets to different channels
        peer.add_packet_to_channel(5, vec![5, 5, 5]);
        peer.add_packet_to_channel(1, vec![1, 1, 1]);

        // Should return from lowest channel (1) first
        let mut buffer = vec![0u8; 10];
        let result = peer.get_packet(buffer.as_mut_slice());
        assert!(result.is_ok());
        assert_eq!(&buffer[..3], &[1, 1, 1]);
    }

    #[test]
    fn test_channel_priority_order() {
        let peer = TestPeer::new();

        // Add packets to channels 10, 3, and 7
        peer.add_packet_to_channel(10, vec![100]);
        peer.add_packet_to_channel(3, vec![30]);
        peer.add_packet_to_channel(7, vec![70]);

        // Should serve channel 3 first (lowest number)
        let mut buffer = vec![0u8; 10];

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer[..1], &[30]);

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer[..1], &[70]);

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer[..1], &[100]);
    }

    #[test]
    fn test_empty_queues_returns_error() {
        let peer = TestPeer::new();
        let mut buffer = vec![0u8; 10];
        let result = peer.get_packet(buffer.as_mut_slice());
        assert!(result.is_err());
    }

    #[test]
    fn test_packet_count_aggregation() {
        let peer = TestPeer::new();

        // Add packets to multiple channels
        peer.add_packet_to_channel(1, vec![10]);
        peer.add_packet_to_channel(1, vec![11]);
        peer.add_packet_to_channel(5, vec![50]);
        peer.add_packet_to_channel(5, vec![51]);
        peer.add_packet_to_channel(5, vec![52]);

        assert_eq!(peer.get_available_packet_count(), 5);
    }

    #[rstest]
    #[case(0, &[1, 2, 3, 4, 5])]
    #[case(100, &[10, 20, 30])]
    #[case(255, &[255])]
    fn test_channel_range_support(#[case] channel_id: i32, #[case] data: &[u8]) {
        let peer = TestPeer::new();

        peer.add_packet_to_channel(channel_id, data.to_vec());

        let mut buffer = vec![0u8; data.len()];
        let result = peer.get_packet(buffer.as_mut_slice());
        assert!(result.is_ok());
        assert_eq!(&buffer[..data.len()], data);
    }

    #[test]
    fn test_multiple_packets_per_channel() {
        let peer = TestPeer::new();

        // Add multiple packets to same channel
        peer.add_packet_to_channel(1, vec![1, 1]);
        peer.add_packet_to_channel(1, vec![1, 2]);
        peer.add_packet_to_channel(1, vec![1, 3]);

        let mut buffer = vec![0u8; 2];

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer, &[1, 1]);

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer, &[1, 2]);

        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer, &[1, 3]);
    }

    #[test]
    fn test_buffer_size_handling() {
        let peer = TestPeer::new();

        // Add packet larger than buffer
        peer.add_packet_to_channel(0, vec![1, 2, 3, 4, 5]);

        // Small buffer should be filled up to capacity
        let mut small_buffer = vec![0u8; 3];
        peer.get_packet(small_buffer.as_mut_slice()).unwrap();
        assert_eq!(&small_buffer, &[1, 2, 3]);
    }

    #[test]
    fn test_no_head_of_line_blocking() {
        let peer = TestPeer::new();

        // Simulate HOL blocking scenario: slow channel should not block fast channel
        // Many packets in high-numbered channel
        for i in 0..100 {
            peer.add_packet_to_channel(10, vec![10, i as u8]);
        }

        // Add high-priority packet in low-numbered channel
        peer.add_packet_to_channel(0, vec![0, 99]);

        // Should return low-numbered channel first despite high-numbered having more packets
        let mut buffer = vec![0u8; 2];
        peer.get_packet(buffer.as_mut_slice()).unwrap();
        assert_eq!(&buffer, &[0, 99]);
    }
}
