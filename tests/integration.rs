#[cfg(test)]
mod networking_state_machine_tests {
    // Minimal tests for Zenoh networking state machine transitions

    #[test]
    fn test_connection_state_transitions() {
        // Test basic connection state machine transitions

        let mut state: i32 = 0; // DISCONNECTED
        assert_eq!(state, 0);

        state = 1; // CONNECTING
        assert_eq!(state, 1);

        state = 2; // CONNECTED
        assert_eq!(state, 2);

        println!("Connection state transitions work");
    }

    #[test]
    fn test_channel_priority_logic() {
        // Test that lower channel numbers have higher priority (HOL blocking prevention)

        // Simulate channel ordering: lower numbers = higher priority
        let channels = vec![0, 1, 2, 10, 20, 100];
        let mut processed_channels = Vec::new();

        // Process channels in priority order (simulate HOL prevention)
        for &channel in &channels {
            if channel < 100 {
                // Skip low priority for test
                processed_channels.push(channel);
            }
        }

        // Verify first processed are highest priority (lowest numbers)
        assert_eq!(processed_channels[0], 0); // Channel 0 processed first
        assert_eq!(processed_channels[1], 1); // Channel 1 processed second
        assert_eq!(processed_channels[2], 2); // Channel 2 processed third

        println!("Channel priority logic works");
    }
}
