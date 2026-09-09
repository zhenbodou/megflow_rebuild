// Adapted from MegFlow 95f870bf, flow-message/src/dr.rs.
// Copyright (c) 2019-2021 Megvii Inc. Apache-2.0.
//! 带脏标记与私有数据的载荷包装，保留原版同步/克隆语义。
use std::any::Any;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

pub trait SyncWith<T> {
    fn sync_with(&mut self, dest: T);
}

pub struct Dr<T> {
    private_data: HashMap<String, Box<dyn Any + Send + Sync>>,
    pub(crate) inner: T,
    dirty: bool,
}

impl<T> From<T> for Dr<T> {
    fn from(inner: T) -> Self {
        Self {
            inner,
            private_data: Default::default(),
            dirty: true,
        }
    }
}

impl<T: Clone> Clone for Dr<T> {
    fn clone(&self) -> Self {
        Self {
            // clone the inner data only
            inner: self.inner.clone(),
            // skip private data
            private_data: Default::default(),
            dirty: true,
        }
    }
}

impl<T, U> SyncWith<U> for Dr<T>
where
    T: SyncWith<U>,
{
    fn sync_with(&mut self, dest: U) {
        if self.dirty {
            self.inner.sync_with(dest);
            self.dirty = false;
        }
    }
}

impl<T> Dr<T> {
    pub fn new(inner: T) -> Self {
        inner.into()
    }

    pub fn add_private_data(&mut self, key: String, header: impl 'static + Send + Sync) {
        self.private_data.insert(key, Box::new(header));
    }

    pub fn private_data<U: 'static + Send + Sync>(&self, key: &str) -> Option<&U> {
        self.private_data.get(key).and_then(|x| x.downcast_ref())
    }

    pub fn private_data_mut<U: 'static + Send + Sync>(&mut self, key: &str) -> Option<&mut U> {
        self.private_data
            .get_mut(key)
            .and_then(|x| x.downcast_mut())
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Deref for Dr<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Dr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        &mut self.inner
    }
}
