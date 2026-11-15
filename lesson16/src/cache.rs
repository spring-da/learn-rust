use serde::Serialize;
use std::{collections::{HashMap, VecDeque}, hash::Hash, sync::Mutex, time::{Duration, Instant}};

struct Entry<V> { value: V, expires_at: Instant }

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct CacheStats {
    pub entries: usize,
    pub capacity: usize,
    pub ttl_ms: u128,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub expirations: u64,
}

struct CacheState<K,V> {
    items: HashMap<K,Entry<V>>,
    order: VecDeque<K>,
    stats: CacheStats,
}

impl<K: Eq+Hash,V> CacheState<K,V> {
    fn remove(&mut self,key:&K) {
        self.items.remove(key);
        self.order.retain(|candidate|candidate!=key);
    }

    fn purge_expired(&mut self,now:Instant) {
        let before=self.items.len();
        self.items.retain(|_,entry|now<entry.expires_at);
        self.stats.expirations+=(before-self.items.len()) as u64;
        let items=&self.items;
        self.order.retain(|key|items.contains_key(key));
    }
}

pub struct TtlCache<K,V> {
    state: Mutex<CacheState<K,V>>,
    capacity: usize,
    ttl: Duration,
}

impl<K: Eq+Hash,V> TtlCache<K,V> {
    pub fn new(capacity:usize,ttl:Duration)->Result<Self,&'static str> {
        if capacity==0 || ttl.is_zero() { return Err("cache capacity and TTL must be positive"); }
        Ok(Self { state:Mutex::new(CacheState {items:HashMap::with_capacity(capacity),order:VecDeque::with_capacity(capacity),stats:CacheStats::default()}),capacity,ttl })
    }

    pub fn delete(&self,key:&K) {
        self.state.lock().expect("cache mutex poisoned").remove(key);
    }

    pub fn stats_at(&self,now:Instant)->CacheStats {
        let mut state=self.state.lock().expect("cache mutex poisoned");
        state.purge_expired(now);
        CacheStats { entries:state.items.len(),capacity:self.capacity,ttl_ms:self.ttl.as_millis(),..state.stats }
    }
}

// get 返回拥有所有权的值，离开方法后 MutexGuard 已释放；只在这里要求 V: Clone。
impl<K: Eq+Hash,V: Clone> TtlCache<K,V> {
    pub fn get_at(&self,key:&K,now:Instant)->Option<V> {
        let mut state=self.state.lock().expect("cache mutex poisoned");
        if let Some(entry)=state.items.get(key) {
            if now<entry.expires_at {
                let value=entry.value.clone();
                state.stats.hits+=1;
                return Some(value);
            }
            state.remove(key);
            state.stats.expirations+=1;
        }
        state.stats.misses+=1;
        None
    }
}

// 键同时存在于 HashMap 和 FIFO 队列，所以插入方法需要 K: Clone。
impl<K: Eq+Hash+Clone,V> TtlCache<K,V> {
    pub fn put_at(&self,key:K,value:V,now:Instant)->Result<(),&'static str> {
        let expires_at=now.checked_add(self.ttl).ok_or("TTL exceeds Instant range")?;
        let mut state=self.state.lock().expect("cache mutex poisoned");
        state.purge_expired(now);
        state.remove(&key);
        if state.items.len()==self.capacity {
            let oldest=state.order.pop_front().expect("FIFO entry exists at capacity");
            state.items.remove(&oldest);
            state.stats.evictions+=1;
        }
        state.order.push_back(key.clone());
        state.items.insert(key,Entry {value,expires_at});
        Ok(())
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
