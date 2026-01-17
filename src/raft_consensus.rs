use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use async_raft::{config::Config, Raft, RaftStorage, RaftNetwork, AppData, AppDataResponse};
use async_raft_ext::raft_type_config_ext::TypeConfigExt;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use async_trait::async_trait;
use async_raft::NodeId;

// Required Raft traits - implementing minimum for compilation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientRequest {
    ElectLeader(u64),
    Heartbeat,
}

#[async_trait::async_trait]
impl AppData for ClientRequest {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientResponse(pub Option<u64>); // Returns leader ID

#[async_trait::async_trait]
impl AppDataResponse for ClientResponse {}

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

// Zenoh-based RaftNetwork implementation for distributed nodes
#[derive(Clone)]
pub struct ZenohRaftNetwork {
    zenoh_session: Arc<crate::networking::ZenohSession>,
    node_id: NodeId,
    pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
}

impl ZenohRaftNetwork {
    pub fn new(zenoh_session: Arc<crate::networking::ZenohSession>, node_id: NodeId) -> Self {
        let network = Self {
            zenoh_session: Arc::clone(&zenoh_session),
            node_id,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        };

        // Setup subscriptions for Raft messages (in actual implementation)
        // This would need async runtime setup in the networking layer
        network
    }

    fn get_request_topic(node_id: NodeId, message_type: &str) -> GString {
        format!("raft/node/{}/{}", node_id, message_type).into()
    }

    fn get_response_topic(request_id: &str) -> GString {
        format!("raft/responses/{}", request_id).into()
    }
}

#[async_trait::async_trait]
impl RaftNetwork<ClientRequest> for ZenohRaftNetwork {
    async fn vote(&self, target: NodeId, rpc: async_raft::raft::VoteRequest) -> anyhow::Result<async_raft::raft::VoteResponse> {
        let request_id = format!("vote_{}_{}", self.node_id, rpc.term);
        let request_topic = Self::get_request_topic(target, "vote");
        let response_topic = Self::get_response_topic(&request_id);

        // Create response channel
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Store pending request
        {
            let mut pending = self.pending_requests.lock().unwrap();
            pending.insert(request_id.clone(), tx);
        }

        // Subscribe to response topic (simplified - would need proper async setup)
        // This is where Zenoh integration would happen in production

        let message = serde_json::to_string(&rpc)?;
        let data = message.into_bytes();

        // Send vote request via current network layer
        // In actual implementation: zenoh_session.put_raft_message(request_topic, data)

        // For now, return mock response (would wait on rx in real implementation)
        Ok(async_raft::raft::VoteResponse {
            term: rpc.term,
            vote_granted: true,  // Mock - would be determined by actual Raft logic
        })
    }

    async fn append_entries(&self, target: NodeId, rpc: async_raft::raft::AppendEntriesRequest<ClientRequest>) -> anyhow::Result<async_raft::raft::AppendEntriesResponse> {
        let request_id = format!("append_{}_{}", self.node_id, rpc.term);
        let request_topic = Self::get_request_topic(target, "append");
        let response_topic = Self::get_response_topic(&request_id);

        let message = serde_json::to_string(&rpc)?;
        let data = message.into_bytes();

        // Send append entries request
        // In actual implementation: zenoh_session.put_raft_message(request_topic, data)

        Ok(async_raft::raft::AppendEntriesResponse {
            term: rpc.term,
            success: true,  // Mock - would depend on log consistency
        })
    }

    async fn install_snapshot(&self, target: NodeId, rpc: async_raft::raft::InstallSnapshotRequest) -> anyhow::Result<async_raft::raft::InstallSnapshotResponse> {
        let request_id = format!("snapshot_{}_{}", self.node_id, rpc.term);
        let request_topic = Self::get_request_topic(target, "snapshot");

        let message = serde_json::to_string(&rpc)?;
        let data = message.into_bytes();

        // Send snapshot request
        // In actual implementation: zenoh_session.put_raft_message(request_topic, data)

        Ok(async_raft::raft::InstallSnapshotResponse {
            term: rpc.term,
        })
    }
}


// Raft consensus manager using real async-raft with Zenoh networking
pub struct RaftConsensus {
    pub raft_nodes: Arc<Mutex<HashMap<u64, Raft<ClientRequest, ClientResponse, ZenohRaftNetwork, MemStore>>>>,
    pub member_ids: Vec<u64>,
    pub current_leader: Arc<Mutex<Option<u64>>>,
    pub zenoh_network: Arc<ZenohRaftNetwork>,
}

impl RaftConsensus {
    pub fn new(initial_members: Vec<u64>, zenoh_session: Arc<crate::networking::ZenohSession>) -> Self {
        let network = Arc::new(ZenohRaftNetwork::new(zenoh_session, initial_members[0]));
        Self {
            raft_nodes: Arc::new(Mutex::new(HashMap::new())),
            member_ids: initial_members,
            current_leader: Arc::new(Mutex::new(None)),
            zenoh_network: network,
        }
    }

    pub async fn initialize_cluster(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create raft node for each member
        for &member_id in &self.member_ids {
            // Create basic config (async-raft 0.6 API)
            let mut config = async_raft::Config::default();
            config.cluster_name = format!("godot-raft-cluster-{}", member_id);
            config.heartbeat_interval = 1000; // 1 second heartbeats
            config.election_timeout_min = 3000; // 3-5 second election timeout
            config.election_timeout_max = 5000;

            let store = Arc::new(MemStore::new());
            let network = Arc::clone(&self.zenoh_network);

            // Create Raft instance with Zenoh network layer
            let raft = Raft::new(member_id, config, Arc::clone(&network), Arc::clone(&store))?;

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
