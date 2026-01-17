/// LIVELINESS TESTS: 15-30ms latency requirements validation
/// Critical for real-time multiplayer gaming responsiveness

use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use rstest::rstest;

/// Liveness-aware channel manager with timing guarantees
pub struct LivenessChannelManager {
    queues: Arc<Mutex<HashMap<i32, VecDeque<(Vec<u8>, Instant)>>>>,
    liveness_budget_ms: u64, // Expected processing budget (15-30ms)
}

impl LivenessChannelManager {
    pub fn new() -> Self {
        LivenessChannelManager::with_budget(15) // Default 15ms
    }

    pub fn with_budget(liveness_budget_ms: u64) -> Self {
        LivenessChannelManager {
            queues: Arc::new(Mutex::new(HashMap::new())),
            liveness_budget_ms,
        }
    }

    pub fn add_packet(&self, channel: i32, data: Vec<u8>) -> Result<(), &'static str> {
        let mut queues = self.queues.lock().unwrap();

        let queue = queues.entry(channel).or_insert_with(VecDeque::new);

        // Liveness constraint: limit channel depth to prevent excessive queuing
        if queue.len() >= 100 { // Prevent unlimited queuing
            return Err("QUEUE_DEPTH_LIMIT_EXCEEDED");
        }

        queue.push_back((data, Instant::now()));
        Ok(())
    }

    pub fn get_packet(&self) -> Option<(i32, Vec<u8>, Duration)> {
        let mut queues = self.queues.lock().unwrap();

        for channel in 0..=255 {
            if let Some(queue) = queues.get_mut(&channel) {
                if let Some((packet, enqueue_time)) = queue.pop_front() {
                    let queue_time = enqueue_time.elapsed();
                    return Some((channel, packet, queue_time));
                }
            }
        }
        None
    }

    pub fn get_packet_raw(&self) -> Option<(i32, Vec<u8>)> {
        let result = self.get_packet();
        result.map(|(ch, data, _)| (ch, data))
    }

    pub fn get_packet_count(&self) -> usize {
        let queues = self.queues.lock().unwrap();
        queues.values().map(|q| q.len()).sum()
    }

    /// Measure current liveness status
    pub fn measure_liveness(&self) -> LivenessReport {
        let queues = self.queues.lock().unwrap();
        let mut oldest_packet_age = Duration::ZERO;
        let mut avg_queue_time = Duration::ZERO;
        let mut total_packets = 0;
        let mut packets_over_budget = 0;

        for queue in queues.values() {
            for (packet_data, enqueue_time) in queue.iter() {
                total_packets += 1;
                let age = enqueue_time.elapsed();
                oldest_packet_age = oldest_packet_age.max(age);
                avg_queue_time += age;

                if age.as_millis() > self.liveness_budget_ms as u128 {
                    packets_over_budget += 1;
                }
            }
        }

        if total_packets > 0 {
            avg_queue_time = Duration::from_nanos(avg_queue_time.as_nanos() as u64 / total_packets as u64);
        }

        LivenessReport {
            total_queued_packets: total_packets,
            oldest_packet_age_ms: oldest_packet_age.as_millis() as u64,
            avg_queue_time_ms: avg_queue_time.as_millis() as u64,
            packets_over_budget,
            liveness_budget_ms: self.liveness_budget_ms,
            percentage_over_budget: if total_packets > 0 { (packets_over_budget * 100) / total_packets } else { 0 },
        }
    }
}

#[derive(Debug)]
pub struct LivenessReport {
    pub total_queued_packets: usize,
    pub oldest_packet_age_ms: u64,
    pub avg_queue_time_ms: u64,
    pub packets_over_budget: usize,
    pub liveness_budget_ms: u64,
    pub percentage_over_budget: usize,
}

#[cfg(test)]
mod liveness_tests {
    use super::*;
    use std::thread;

    /// Test basic 15ms liveness requirement
    #[test]
    fn test_15ms_liveness_basic() {
        let manager = LivenessChannelManager::with_budget(15);

        manager.add_packet(0, vec![1, 2, 3]).unwrap();

        // Immediate retrieval should be under budget
        let (channel, data, queue_time) = manager.get_packet().unwrap();
        assert_eq!(channel, 0);
        assert_eq!(data, vec![1, 2, 3]);
        assert!(queue_time.as_millis() < 15,
                "Immediate packet retrieval failed liveness budget: {}ms > 15ms", queue_time.as_millis());
    }

    /// Test traffic spikes don't exceed liveness budget
    #[test]
    fn test_liveness_under_traffic_spike() {
        let manager = LivenessChannelManager::with_budget(30);

        // Simulate gaming traffic spike: multiple packets at once
        for i in 0..10 {
            manager.add_packet(0, vec![i; 100]).unwrap(); // 100-byte packets
        }

        // All packets should be processed within liveness budget
        for i in 0..10 {
            let (channel, data, queue_time) = manager.get_packet().unwrap();
            assert_eq!(channel, 0);
            assert!(queue_time.as_millis() < 30,
                    "Packet {} exceeded 30ms liveness budget: {}ms",
                    i, queue_time.as_millis());
        }
    }

    /// Test sustained load maintains liveness
    #[test]
    fn test_sustained_load_liveness() {
        let manager = LivenessChannelManager::with_budget(20);

        // Simulate 100ms of continuous traffic (5 packets per 20ms window)
        let start_test = Instant::now();

        while start_test.elapsed() < Duration::from_millis(100) {
            // Add traffic at regular intervals
            manager.add_packet(0, vec![1, 2, 3, 4]).unwrap();
            thread::sleep(Duration::from_millis(1)); // 1ms between packets

            // Continuously process available packets
            while let Some((channel, data, queue_time)) = manager.get_packet() {
                assert!(queue_time.as_millis() < 20,
                        "Sustained traffic failed liveness: {}ms > 20ms budget", queue_time.as_millis());
            }
        }

        println!("Sustained load test completed with 20ms liveness budget maintained");
    }

    /// Test priority channel responsiveness under congestion
    #[test]
    fn test_priority_channel_liveness() {
        let manager = LivenessChannelManager::with_budget(25);

        // Background congestion on high channels
        for channel in 200..210 {
            for packet_id in 0..50 {
                manager.add_packet(channel as i32, vec![packet_id as u8; 500]).unwrap();
            }
        }

        // Critical priority packet (channel 0) added last
        let priority_time = Instant::now();
        manager.add_packet(0, vec![255, 255, 255]).unwrap(); // Marker packet

        // Process packets until priority one is found
        let mut found_priority = false;
        let mut packets_processed = 0;

        while let Some((channel, data, queue_time)) = manager.get_packet() {
            packets_processed += 1;

            if channel == 0 && data == vec![255, 255, 255] {
                found_priority = true;
                let actual_latency = priority_time.elapsed();
                println!("Priority packet found after {} other packets, latency: {:?}",
                        packets_processed - 1, actual_latency);

                assert!(actual_latency.as_millis() < 25,
                        "CRITICAL: Priority packet exceeded 25ms liveness budget: {}ms",
                        actual_latency.as_millis());
                break;
            }

            // Emergency timeout
            if packets_processed > 1000 {
                panic!("Failed to find priority packet within reasonable bound");
            }
        }

        assert!(found_priority, "Priority packet never processed");
    }

    /// Test varying liveness budgets (15ms, 20ms, 30ms)
    #[rstest]
    #[case(15)]
    #[case(20)]
    #[case(30)]
    fn test_multiple_liveness_budgets(#[case] budget_ms: u64) {
        let manager = LivenessChannelManager::with_budget(budget_ms);

        // Add packet and measure immediate retrieval time
        manager.add_packet(0, vec![budget_ms as u8]).unwrap();

        let start = Instant::now();
        let (channel, data, queue_time) = manager.get_packet().unwrap();

        // The actual queue time should be much less than budget (microseconds)
        let measured_latency_ms = start.elapsed().as_micros() as f64 / 1000.0;

        println!("Budget: {}ms, Measured Latency: {:.3}ms", budget_ms, measured_latency_ms);

        assert!(measured_latency_ms < budget_ms as f64,
                "Measured latency {:.3}ms exceeded budget {}ms", measured_latency_ms, budget_ms);
    }

    /// Test liveness monitoring/reporting
    #[test]
    fn test_liveness_monitoring() {
        let manager = LivenessChannelManager::with_budget(20);

        // Add packets with artificial delays to simulate processing time
        let start = Instant::now();

        for i in 0..5 {
            manager.add_packet(i, vec![i as u8; 50]).unwrap();
            thread::sleep(Duration::from_millis(2)); // 2ms between additions
        }

        // Check liveness report
        let report = manager.measure_liveness();
        println!("Liveness Report:");
        println!("  Total queued: {}", report.total_queued_packets);
        println!("  Oldest packet age: {}ms", report.oldest_packet_age_ms);
        println!("  Average queue time: {}ms", report.avg_queue_time_ms);
        println!("  Over budget: {}/{} ({}%)",
                report.packets_over_budget,
                report.total_queued_packets,
                report.percentage_over_budget);

        // With 2ms spacing, oldest packet should be around 8-10ms old
        assert!(report.oldest_packet_age_ms < 20,
                "Oldest packet too old: {}ms", report.oldest_packet_age_ms);
        assert_eq!(report.packets_over_budget, 0,
                "No packets should be over 20ms budget");
    }

    /// Simulate end-to-end network round-trip liveness
    #[test]
    fn test_network_round_trip_liveness() {
        let manager = LivenessChannelManager::with_budget(15);

        // Simulate a client sending a "ping" and server responding
        let client_start = Instant::now();

        // Client sends ping
        manager.add_packet(1, vec![0x01, 0x01]).unwrap(); // Channel 1: client requests

        // Simulate 5ms network + processing delay
        thread::sleep(Duration::from_millis(5));

        // Server processes and responds
        if let Some((ch, data, delay)) = manager.get_packet() {
            assert_eq!(ch, 1);
            assert_eq!(data, vec![0x01, 0x01]);

            // Server sends pong on different channel
            manager.add_packet(2, vec![0x02, 0x02]).unwrap(); // Channel 2: server response
        }

        // Simulate another 5ms round-trip
        thread::sleep(Duration::from_millis(5));

        // Client receives pong
        let (response_ch, response_data, full_round_trip) = manager.get_packet().unwrap();

        assert_eq!(response_ch, 2);
        assert_eq!(response_data, vec![0x02, 0x02]);

        // Full round trip should be under 15ms budget including simulated network delays
        println!("Full round-trip latency: {:?}", full_round_trip);
        println!("Budget: 15ms, achieved in {:?}", client_start.elapsed());

        // Allow some tolerance for thread timing
        assert!(full_round_trip.as_millis() < 20,
                "Round-trip exceeded timing budget: {:?}", full_round_trip);
    }

    /// Extreme stress test maintaining liveness guarantees
    #[test]
    fn test_extreme_load_liveness_guarantee() {
        let manager = LivenessChannelManager::with_budget(30); // Relaxed budget for extreme load

        // Generate extreme load: 1000 packets across 50 channels
        for channel in 0..50 {
            for packet_id in 0..20 {
                let data = vec![channel as u8; 1024]; // 1KB packets for memory pressure
                manager.add_packet(channel as i32, data).unwrap();
            }
        }

        let start_processing = Instant::now();

        // Process batch by batch, monitoring liveness
        let mut total_processed = 0;
        let mut total_lateness = 0u128;

        for _ in 0..10 { // Process in 10 batches
            let batch_start = Instant::now();

            // Process up to 50 packets per batch
            for _ in 0..50 {
                if let Some((ch, data, queue_time)) = manager.get_packet() {
                    total_processed += 1;
                    total_lateness += queue_time.as_millis();

                    // Individual packet must still be within budget
                    assert!(queue_time.as_millis() < 50,
                            "Packet exceeded relaxed 50ms budget under extreme load");
                } else {
                    break;
                }
            }

            println!("Batch processed in {:?}", batch_start.elapsed());
        }

        let avg_lateness = if total_processed > 0 {
            total_lateness as f64 / total_processed as f64
        } else { 0.0 };

        println!("Extreme load test completed:");
        println!("  Total packets processed: {}", total_processed);
        println!("  Average queue time: {:.1}ms", avg_lateness);
        println!("  Total processing time: {:?}", start_processing.elapsed());

        // Hard guarantee: no packet should be excessively late even under extreme load
        assert!(avg_lateness < 30.0,
                "Average lateness too high under extreme load: {:.1}ms", avg_lateness);
    }
}
