use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use async_raft::{config::Config, Raft, RaftStorage};
use async_raft_ext::raft_type_config_ext::TypeConfigExt;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

// Raft type configuration for multiplayer coordination
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientRequest {
    ElectLeader(u64),
    Heartbeat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientResponse(pub Option<u64>); // Returns leader ID

// Network message types for Raft communication
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RaftMessage {
    VoteRequest {
        candidate_id: u64,
        term: u64,
        last_log_index: u64,
        last_log_term: u64,
    },
    VoteResponse {
        voter_id: u64,
        term: u64,
        vote_granted: bool,
    },
    AppendEntries {
        leader_id: u64,
        term: u64,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    },
    AppendResponse {
        follower_id: u64,
        term: u64,
        success: bool,
        match_index: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub command: ClientRequest,
}

// Memory-based Raft storage implementation
#[derive(Clone)]
pub struct MemStore {
    pub hard_state: Arc<Mutex<Option<async_raft::storage::HardState<ClientRequest>>>>,
    pub log: Arc<Mutex<BTreeMap<u64, LogEntry>>>,
    pub current_term: u64,
    pub voted_for: Option<u64>,
    pub last_applied: u64,
    pub commit_index: u64,
}

impl MemStore {
    pub fn new() -> Self {
        Self {
            hard_state: Arc::new(Mutex::new(None)),
            log: Arc::new(Mutex::new(BTreeMap::new())),
            current_term: 0,
            voted_for: None,
            last_applied: 0,
            commit_index: 0,
        }
    }

    pub fn get_log_term(&self, index: u64) -> Option<u64> {
        self.log.lock().unwrap().get(&index).map(|entry| entry.term)
    }

    pub fn get_log_at(&self, index: u64) -> Option<LogEntry> {
        self.log.lock().unwrap().get(&index).cloned()
    }

    pub fn get_last_log_entry(&self) -> Option<(u64, LogEntry)> {
        self.log.lock().unwrap().last_key_value().map(|(k, v)| (*k, v.clone()))
    }
}

// Full async-raft RaftStorage implementation
#[async_trait::async_trait]
impl RaftStorage<ClientRequest, ClientResponse> for MemStore {
    type SnapshotData = Cursor<Vec<u8>>;
    type SnapshotMeta = String;

    async fn get_current_term(&self) -> Result<u64, async_raft::storage::StorageError> {
        Ok(self.current_term)
    }

    async fn set_current_term(&self, term: u64) -> Result<(), async_raft::storage::StorageError> {
        self.current_term = term;
        Ok(())
    }

    async fn poll_read(&self) -> Result<Option<Vec<LogEntry>>, async_raft::storage::StorageError> {
        // Return entries from last commit index + 1
        let mut entries = Vec::new();
        let log = self.log.lock().unwrap();
        for (_, entry) in log.range((self.last_applied + 1)..) {
            entries.push(entry.clone());
        }
        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(entries))
        }
    }

    async fn append(&self, entries: &[&LogEntry]) -> Result<(), async_raft::storage::StorageError> {
        let mut log = self.log.lock().unwrap();
        for entry in entries {
            log.insert(entry.index, (*entry).clone());
        }
        Ok(())
    }

    async fn snapshot(&self) -> Result<Self::SnapshotData, async_raft::storage::StorageError> {
        Ok(Cursor::new(Vec::new()))
    }

    async fn get_log_state(&self) -> Result<(u64, u64), async_raft::storage::StorageError> {
        let log = self.log.lock().unwrap();
        match log.last_key_value() {
            Some((index, entry)) => Ok((*index, entry.term)),
            None => Ok((0, 0)),
        }
    }

    async fn save_hard_state(&self, hs: &async_raft::storage::HardState<ClientRequest>) -> Result<(), async_raft::storage::StorageError> {
        *self.hard_state.lock().unwrap() = Some(hs.clone());
        self.current_term = hs.current_term;
        self.voted_for = hs.voted_for;
        self.commit_index = hs.commit_index;
        Ok(())
    }

    async fn apply_to_state_machine(&self, entries: &[&LogEntry]) -> Result<Vec<ClientResponse>, async_raft::storage::StorageError> {
        let mut responses = Vec::new();
        for entry in entries {
            match &entry.command {
                ClientRequest::ElectLeader(leader_id) => {
                    responses.push(ClientResponse(Some(*leader_id)));
                    godot_print!("RAFT: Applied leader election for {}", leader_id);
                }
                ClientRequest::Heartbeat => {
                    responses.push(ClientResponse(None));
                }
            }
            self.last_applied = entry.index;
        }
        Ok(responses)
    }

    async fn get_last_purged_log_id(&self) -> Result<u64, async_raft::storage::StorageError> {
        Ok(0)
    }
}

// Raft consensus manager using real async-raft
pub struct RaftConsensus {
    pub raft_nodes: Arc<Mutex<HashMap<u64, Raft<ClientRequest, ClientResponse>>>>,
    pub member_ids: Vec<u64>,
    pub current_leader: Arc<Mutex<Option<u64>>>,
}

impl RaftConsensus {
    pub fn new(initial_members: Vec<u64>) -> Self {
        Self {
            raft_nodes: Arc::new(Mutex::new(HashMap::new())),
            member_ids: initial_members,
            current_leader: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn initialize_cluster(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create raft node for each member
        for &member_id in &self.member_ids {
            let config = Config::default();
            config.cluster_name.clone_from(&"godot-multiplayer".to_string());
            config.heartbeat_interval = 1000; // 1 second
            config.election_timeout_min = 3000; // 3 seconds
            config.election_timeout_max = 5000; // 5 seconds

            let store = Arc::new(MemStore::new());

            // TODO: Implement proper network transport for Raft
            // For now, this will fail to compile because we need the network layer
            let raft = Raft::new(member_id, config, store, /* network */ todo!()).await?;

            // Initialize the raft cluster
            raft.initialize(self.member_ids.clone()).await?;

            self.raft_nodes.lock().unwrap().insert(member_id, raft);
        }

        Ok(())
    }

    pub async fn get_leader(&self) -> Option<u64> {
        *self.current_leader.lock().unwrap()
    }

    pub async fn propose_election(&self, proposer_id: u64, candidate_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(raft) = self.raft_nodes.lock().unwrap().get(&proposer_id) {
            let request = ClientRequest::ElectLeader(candidate_id);
            raft.client_write(request).await?;
        }
        Ok(())
    }

    pub async fn send_heartbeat(&self, from_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(raft) = self.raft_nodes.lock().unwrap().get(&from_id) {
            let request = ClientRequest::Heartbeat;
            raft.client_write(request).await?;
        }
        Ok(())
    }
}
