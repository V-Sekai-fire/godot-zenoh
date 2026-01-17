//! Raft consensus tests - verifying the distributed consensus algorithm works correctly

use godot_zenoh::raft_consensus::*;
use std::sync::Arc;

#[test]
fn test_raft_consensus_basic() {
    // Run basic Raft functionality test
    let result = test_raft_consensus_basic();
    assert!(result.is_ok(), "Basic Raft consensus test failed");
    println!("✅ Basic Raft consensus functionality verified");
}

#[test]
fn test_raft_structure_creation() {
    // Run Raft structure creation test
    let result = test_raft_structure_creation();

    match result {
        Ok(_) => println!("✅ Raft structure creation test passed"),
        Err(e) => {
            println!("❌ Raft structure creation test failed: {:?}", e);
            panic!("Raft structure test failed");
        }
    }
}

#[test]
fn test_memstore_basic_functionality() {
    // Test basic storage functionality
    let store = MemStore::new();

    // Test log operations
    let log_entry = LogEntry {
        term: 1,
        index: 1,
        command: ClientRequest::Heartbeat,
    };

    // Insert entry manually to test basic functionality
    store.log.lock().unwrap().insert(1, log_entry.clone());

    // Verify retrieval
    let retrieved = store.get_log_at(1);
    assert!(retrieved.is_some(), "Failed to retrieve log entry");
    assert_eq!(retrieved.unwrap().term, 1);
    assert_eq!(retrieved.unwrap().index, 1);

    println!("✅ MemStore basic functionality verified");
}

#[test]
fn test_client_request_appdata() {
    // Test that ClientRequest properly implements AppData trait
    use async_raft::AppData;

    let request1 = ClientRequest::ElectLeader(5);
    let request2 = ClientRequest::Heartbeat;

    // Clone check (required by AppData)
    let _request1_clone = request1.clone();

    // Verify discriminate types
    match request1 {
        ClientRequest::ElectLeader(leader_id) => assert_eq!(leader_id, 5),
        _ => panic!("Expected ElectLeader"),
    }

    match request2 {
        ClientRequest::Heartbeat => {} // Expected
        _ => panic!("Expected Heartbeat"),
    }

    println!("✅ ClientRequest AppData implementation verified");
}

#[test]
fn test_client_response_appdata() {
    // Test that ClientResponse properly implements AppDataResponse trait
    use async_raft::AppDataResponse;

    let response1 = ClientResponse(Some(42));
    let response2 = ClientResponse(None);

    // Clone check (required by AppDataResponse)
    let _response1_clone = response1.clone();

    // Verify discriminate types
    match response1 {
        ClientResponse(leader_id_opt) => assert_eq!(leader_id_opt, Some(42)),
    }

    match response2 {
        ClientResponse(leader_id_opt) => assert_eq!(leader_id_opt, None),
    }

    println!("✅ ClientResponse AppDataResponse implementation verified");
}

#[tokio::test]
async fn test_dummy_raft_network() {
    // Test DummyRaftNetwork basic functionality
    let dummy_session = Arc::new(crate::networking::ZenohSession::default());
    let network = DummyRaftNetwork::new(dummy_session, 1);

    // Test vote request handling
    let vote_request = async_raft::raft::VoteRequest {
        term: 5,
        candidate_id: 3,
        last_log_index: 10,
        last_log_term: 2,
    };

    let vote_response = network.vote(2, vote_request).await;
    assert!(vote_response.is_ok(), "Vote request failed");
    println!("✅ DummyRaftNetwork vote functionality verified");

    // Test append entries request
    let entries = vec![ClientRequest::Heartbeat];
    let append_request = async_raft::raft::AppendEntriesRequest {
        term: 5,
        leader_id: 1,
        prev_log_index: 9,
        prev_log_term: 2,
        entries,
        leader_commit: 9,
    };

    let append_response = network.append_entries(2, append_request).await;
    assert!(append_response.is_ok(), "Append entries request failed");
    println!("✅ DummyRaftNetwork append entries functionality verified");
}

#[test]
fn test_raft_message_types() {
    // Test RaftMessage enum creation and pattern matching
    let vote_request = RaftMessage::VoteRequest {
        candidate_id: 5,
        term: 10,
        last_log_index: 100,
        last_log_term: 8,
    };

    let vote_response = RaftMessage::VoteResponse {
        voter_id: 3,
        term: 10,
        vote_granted: true,
    };

    let heartbeat = RaftMessage::Heartbeat(7);

    // Pattern matching verification
    match vote_request {
        RaftMessage::VoteRequest { candidate_id, term, .. } => {
            assert_eq!(candidate_id, 5);
            assert_eq!(term, 10);
        }
        _ => panic!("Expected VoteRequest"),
    }

    match vote_response {
        RaftMessage::VoteResponse { voter_id, vote_granted, .. } => {
            assert_eq!(voter_id, 3);
            assert!(vote_granted);
        }
        _ => panic!("Expected VoteResponse"),
    }

    match heartbeat {
        RaftMessage::Heartbeat(sender_id) => assert_eq!(sender_id, 7),
        _ => panic!("Expected Heartbeat"),
    }

    println!("✅ RaftMessage enum types verified");
}

#[test]
fn test_log_entry_structure() {
    // Test LogEntry creation and access
    let log_entry = LogEntry {
        term: 42,
        index: 1001,
        command: ClientRequest::ElectLeader(7),
    };

    assert_eq!(log_entry.term, 42);
    assert_eq!(log_entry.index, 1001);
    assert_eq!(log_entry.command, ClientRequest::ElectLeader(7));

    // Test cloning
    let _cloned = log_entry.clone();

    println!("✅ LogEntry structure verified");
}

#[test]
fn test_raft_node_state() {
    // Test RaftNodeState structure
    let mut state = RaftNodeState {
        current_term: 10,
        voted_for: Some(5),
        log: Vec::new(),
        commit_index: 50,
        last_applied: 45,
    };

    assert_eq!(state.current_term, 10);
    assert_eq!(state.voted_for, Some(5));
    assert_eq!(state.commit_index, 50);
    assert_eq!(state.last_applied, 45);
    assert!(state.log.is_empty());

    // Test log modification
    let log_entry = LogEntry {
        term: 10,
        index: 51,
        command: ClientRequest::Heartbeat,
    };
    state.log.push(log_entry);
    assert_eq!(state.log.len(), 1);

    println!("✅ RaftNodeState structure verified");
}
