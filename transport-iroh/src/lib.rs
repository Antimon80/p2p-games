use anyhow::Result;

#[async_trait::async_trait]
pub trait GossipTransport: Send + Sync {
    async fn join_topic(&self, topic: &str) -> Result<Box<dyn TopicHandle>>;
}

#[async_trait::async_trait]
pub trait TopicHandle: Send + Sync {
    async fn publish(&self, bytes: &[u8]) -> Result<()>;
    async fn next(&self, bytes: &[u8]) -> Result<Vec<u8>>;
}

pub struct IrohTransport;

impl IrohTransport {
    pub async fn new_in_memory() -> Result<Self> {
        Ok(Self)
    }
}

#[async_trait::async_trait]
impl GossipTransport for IrohTransport {
    async fn join_topic(&self, _topic: &str) -> Result<Box<dyn TopicHandle>> {
        Err(anyhow::anyhow!("Iroh transport not implemented yet"))
    }
}