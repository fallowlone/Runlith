use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use memory::model::{
    Entity, EntityId, EntityLink, MemoryError, MemoryId, MemoryLink, MemoryRecord,
};
use memory::port::{CaptureOutcome, MemoryStorePort};
use memory_contract::run_memory_store_contract;

#[derive(Clone, Default)]
struct MemStore {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    records: BTreeMap<MemoryId, MemoryRecord>,
    links: Vec<MemoryLink>,
    entities: BTreeMap<EntityId, Entity>,
    entity_links: Vec<EntityLink>,
}

impl MemoryStorePort for MemStore {
    fn capture(&mut self, record: &MemoryRecord) -> Result<CaptureOutcome, MemoryError> {
        record.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.records.get(&record.id) {
            if existing == record {
                return Ok(CaptureOutcome {
                    record: existing.clone(),
                    deduplicated: true,
                });
            }
            return Err(MemoryError::Conflict(record.id.to_string()));
        }
        if let Some(existing) = inner
            .records
            .values()
            .find(|r| r.dedup_key() == record.dedup_key())
        {
            return Ok(CaptureOutcome {
                record: existing.clone(),
                deduplicated: true,
            });
        }
        inner.records.insert(record.id.clone(), record.clone());
        Ok(CaptureOutcome {
            record: record.clone(),
            deduplicated: false,
        })
    }

    fn load(&self, id: &MemoryId) -> Result<Option<MemoryRecord>, MemoryError> {
        Ok(self.inner.lock().unwrap().records.get(id).cloned())
    }

    fn load_by_dedup_key(&self, key: &str) -> Result<Option<MemoryRecord>, MemoryError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .records
            .values()
            .find(|r| r.dedup_key() == key)
            .cloned())
    }

    fn update(&mut self, record: &MemoryRecord) -> Result<(), MemoryError> {
        record.revalidate()?;
        let mut inner = self.inner.lock().unwrap();
        if !inner.records.contains_key(&record.id) {
            return Err(MemoryError::NotFound(record.id.to_string()));
        }
        inner.records.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn link(&mut self, link: &MemoryLink) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.records.contains_key(&link.from) || !inner.records.contains_key(&link.to) {
            return Err(MemoryError::NotFound("link end".to_owned()));
        }
        if !inner.links.contains(link) {
            inner.links.push(link.clone());
        }
        Ok(())
    }

    fn links_from(&self, from: &MemoryId) -> Result<Vec<MemoryLink>, MemoryError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .links
            .iter()
            .filter(|l| &l.from == from)
            .cloned()
            .collect())
    }

    fn upsert_entity(&mut self, entity: &Entity) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.entities.get(&entity.id) {
            if existing != entity {
                return Err(MemoryError::Conflict(entity.id.to_string()));
            }
            return Ok(());
        }
        inner.entities.insert(entity.id.clone(), entity.clone());
        Ok(())
    }

    fn load_entity(&self, id: &EntityId) -> Result<Option<Entity>, MemoryError> {
        Ok(self.inner.lock().unwrap().entities.get(id).cloned())
    }

    fn link_entity(&mut self, link: &EntityLink) -> Result<(), MemoryError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.records.contains_key(&link.memory) || !inner.entities.contains_key(&link.entity) {
            return Err(MemoryError::NotFound("entity link end".to_owned()));
        }
        if !inner.entity_links.contains(link) {
            inner.entity_links.push(link.clone());
        }
        Ok(())
    }

    fn entities_for(&self, memory_id: &MemoryId) -> Result<Vec<Entity>, MemoryError> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<Entity> = inner
            .entity_links
            .iter()
            .filter(|l| &l.memory == memory_id)
            .filter_map(|l| inner.entities.get(&l.entity).cloned())
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    fn list_eligible(&self, now: u64) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .records
            .values()
            .filter(|r| r.is_context_eligible(now))
            .cloned()
            .collect())
    }
}

#[test]
fn in_memory_store_satisfies_contract() {
    let mut store = MemStore::default();
    let shared = store.clone();
    run_memory_store_contract(&mut store, move || shared.clone());
}
