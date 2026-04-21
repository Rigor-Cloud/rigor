use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

use uuid::Uuid;

// ---------------------------------------------------------------------------
// ConversationId
// ---------------------------------------------------------------------------

/// Identifies a conversation. May originate from an explicit session id,
/// a fingerprint hash, or be anonymous.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ConversationId(pub String);

impl ConversationId {
    /// Build from an explicit session identifier (stored as-is).
    pub fn from_session(session_id: &str) -> Self {
        Self(session_id.to_string())
    }

    /// Build from a fingerprint hash (prefixed with `"fp:"`).
    pub fn from_fingerprint(hash: &str) -> Self {
        Self(format!("fp:{hash}"))
    }

    /// Create an anonymous conversation id (random UUID v4).
    pub fn anonymous() -> Self {
        Self(format!("anon:{}", Uuid::new_v4()))
    }
}

// ---------------------------------------------------------------------------
// RequestId
// ---------------------------------------------------------------------------

/// Unique identifier for a single HTTP request/response exchange.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

// ---------------------------------------------------------------------------
// ConversationCtx
// ---------------------------------------------------------------------------

/// Shared mutable state for one request-response exchange inside the egress
/// pipeline. The `scratch` map provides typed, heterogeneous storage so that
/// pipeline stages can pass arbitrary data to downstream stages without
/// coupling their concrete types at compile time.
pub struct ConversationCtx {
    pub conversation_id: ConversationId,
    pub request_id: RequestId,
    scratch: HashMap<TypeId, Box<dyn Any + Send>>,
}

impl ConversationCtx {
    /// Full constructor.
    pub fn new(conversation_id: ConversationId, request_id: RequestId) -> Self {
        Self {
            conversation_id,
            request_id,
            scratch: HashMap::new(),
        }
    }

    /// Convenience constructor with an anonymous conversation id and a random
    /// request id.
    pub fn new_anonymous() -> Self {
        Self::new(
            ConversationId::anonymous(),
            RequestId(Uuid::new_v4().to_string()),
        )
    }

    /// Store a value in the scratch map, keyed by its concrete type.
    /// Overwrites any previous value of the same type.
    pub fn scratch_set<T: Any + Send + 'static>(&mut self, val: T) {
        self.scratch.insert(TypeId::of::<T>(), Box::new(val));
    }

    /// Retrieve an immutable reference to a previously stored value.
    pub fn scratch_get<T: Any + Send + 'static>(&self) -> Option<&T> {
        self.scratch
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }

    /// Retrieve a mutable reference to a previously stored value.
    pub fn scratch_get_mut<T: Any + Send + 'static>(&mut self) -> Option<&mut T> {
        self.scratch
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }

    /// Return a mutable reference to the value of type `T`, inserting a new
    /// one created by `f` if it does not already exist.
    pub fn scratch_get_or_insert_with<T: Any + Send + 'static>(
        &mut self,
        f: impl FnOnce() -> T,
    ) -> &mut T {
        self.scratch
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(f()))
            .downcast_mut::<T>()
            .expect("type mismatch in scratch_get_or_insert_with (should be unreachable)")
    }
}

impl fmt::Debug for ConversationCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConversationCtx")
            .field("conversation_id", &self.conversation_id)
            .field("request_id", &self.request_id)
            .field("scratch_keys", &self.scratch.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_id_variants() {
        let session = ConversationId::from_session("sess-42");
        assert_eq!(session.0, "sess-42");

        let fp = ConversationId::from_fingerprint("abc123");
        assert!(
            fp.0.starts_with("fp:"),
            "fingerprint id should start with 'fp:'"
        );
        assert_eq!(fp.0, "fp:abc123");

        let anon = ConversationId::anonymous();
        assert!(
            anon.0.starts_with("anon:"),
            "anonymous id should start with 'anon:'"
        );
        // Each anonymous id must be unique.
        let anon2 = ConversationId::anonymous();
        assert_ne!(anon, anon2);
    }

    #[test]
    fn scratch_set_and_get() {
        let mut ctx = ConversationCtx::new_anonymous();
        ctx.scratch_set("hello".to_string());

        let val = ctx.scratch_get::<String>();
        assert_eq!(val, Some(&"hello".to_string()));
    }

    #[test]
    fn scratch_different_types_coexist() {
        let mut ctx = ConversationCtx::new_anonymous();
        ctx.scratch_set("a_string".to_string());
        ctx.scratch_set(42u32);

        assert_eq!(ctx.scratch_get::<String>(), Some(&"a_string".to_string()));
        assert_eq!(ctx.scratch_get::<u32>(), Some(&42u32));
    }

    #[test]
    fn scratch_get_or_insert_with() {
        let mut ctx = ConversationCtx::new_anonymous();

        // First call should initialise.
        let vec = ctx.scratch_get_or_insert_with::<Vec<i32>>(Vec::new);
        vec.push(1);

        // Second call should return the existing vec (now containing [1]).
        let vec = ctx.scratch_get_or_insert_with::<Vec<i32>>(Vec::new);
        vec.push(2);

        let vec = ctx.scratch_get::<Vec<i32>>().unwrap();
        assert_eq!(vec, &vec![1, 2], "vec should have grown across calls");
    }

    #[test]
    fn scratch_get_mut() {
        let mut ctx = ConversationCtx::new_anonymous();
        ctx.scratch_set(vec![1, 2, 3]);

        let v = ctx.scratch_get_mut::<Vec<i32>>().unwrap();
        v.push(4);

        let v = ctx.scratch_get::<Vec<i32>>().unwrap();
        assert_eq!(v, &vec![1, 2, 3, 4]);
    }

    #[test]
    fn scratch_missing_type_returns_none() {
        let ctx = ConversationCtx::new_anonymous();
        assert!(ctx.scratch_get::<String>().is_none());
        assert!(ctx.scratch_get::<u64>().is_none());
    }
}
