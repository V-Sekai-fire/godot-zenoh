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

// Property-based tests for statistical validation
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_channel_ordering_property(channels in prop::collection::vec(0..=255u32, 1..20)) {
            let peer = TestPeer::new();

            // Add one packet per channel with channel number as data
            for &channel in &channels {
                let channel_id = channel as i32;
                peer.add_packet_to_channel(channel_id, vec![channel_id as u8, 0]);
            }

            // Extract packets - should come out in ascending channel order
            let mut extracted_channels = Vec::new();
            let mut buffer = vec![0u8; 2];

            for _ in 0..channels.len() {
                if let Ok(_) = peer.get_packet(buffer.as_mut_slice()) {
                    extracted_channels.push(buffer[0] as i32);
                }
            }

            // Verify ordering: extracted channels should be sorted ascending
            prop_assert_eq!(extracted_channels, {
                let mut expected = channels.clone();
                expected.sort();
                expected.into_iter().map(|c| c as i32).collect::<Vec<_>>()
            });
        }

        #[test]
        fn test_packet_integrity(channel in 0..=255u32, data in prop::collection::vec(0..=255u8, 1..100)) {
            let peer = TestPeer::new();
            let channel_id = channel as i32;

            // Add packet to channel
            peer.add_packet_to_channel(channel_id, data.clone());

            // Retrieve packet
            let mut buffer = vec![0u8; data.len()];
            peer.get_packet(buffer.as_mut_slice()).unwrap();

            // Verify data integrity
            prop_assert_eq!(&buffer[..data.len()], &data[..]);
        }

        #[test]
        fn test_channel_workload_simulation(
            low_channel_packets in prop::collection::vec(prop::collection::vec(0..=255u8, 1..10), 10..50),
            high_channel_packets in prop::collection::vec(prop::collection::vec(0..=255u8, 1..10), 100..200)
        ) {
            let peer = TestPeer::new();

            // Simulate high workload on high channel (channel 255)
            for packet in &high_channel_packets {
                peer.add_packet_to_channel(255, packet.clone());
            }

            // Add some packets to low channel (channel 0)
            for packet in &low_channel_packets {
                peer.add_packet_to_channel(0, packet.clone());
            }

            // Low channel packets should always be served first (HOL blocking prevention)
            let total_low_packets = low_channel_packets.len();
            let mut buffer = vec![0u8; 10];

            for i in 0..total_low_packets.min(5) {  // Check first few packets
                peer.get_packet(buffer.as_mut_slice()).unwrap();
                // Should be from low channel (channel 0)
                prop_assert_eq!(buffer[0], 0, "Received packet from wrong channel in HOL test");
            }
        }

        #[test]
        fn test_fifo_within_channel(packets in prop::collection::vec(prop::collection::vec(0..=255u8, 3..8), 2..20)) {
            let peer = TestPeer::new();
            let channel = 42;

            // Add multiple packets to same channel
            let mut sent_packets = Vec::new();
            for packet in &packets {
                peer.add_packet_to_channel(channel, packet.clone());
                sent_packets.push(packet.clone());
            }

            // Retrieve all packets - should maintain FIFO order
            let mut received_packets = Vec::new();
            let mut buffer = vec![0u8; 10];

            for _ in 0..packets.len() {
                peer.get_packet(buffer.as_mut_slice()).unwrap();
                let received_len = buffer.iter().position(|&x| x == 0).unwrap_or(buffer.len());
                received_packets.push(buffer[..received_len].to_vec());
            }

            // Should receive packets in exact order sent
            prop_assert_eq!(received_packets, sent_packets);
        }

        #[test]
        fn test_buffer_bounds_safety(packet_size in 1..1000usize, buffer_size in 1..100usize) {
            let peer = TestPeer::new();
            let channel = 5;

            // Create packet that may be larger than buffer
            let packet_data: Vec<u8> = (0..packet_size).map(|i| (i % 256) as u8).collect();

            peer.add_packet_to_channel(channel, packet_data.clone());

            // Retrieve with smaller buffer
            let mut buffer = vec![0u8; buffer_size];

            // Should not panic and should copy min(len, buffer.len()) bytes
            let result = peer.get_packet(buffer.as_mut_slice());
            prop_assert!(result.is_ok());

            let expected_copied = std::cmp::min(packet_size, buffer_size);
            prop_assert_eq!(&buffer[..expected_copied], &packet_data[..expected_copied]);

            // Rest of buffer should be unchanged (0)
            for &byte in &buffer[expected_copied..] {
                prop_assert_eq!(byte, 0);
            }
        }
    }
}
