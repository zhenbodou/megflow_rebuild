/*
 * \file flow-rs/src/config/interlayer.rs
 * MegFlow is Licensed under the Apache License, Version 2.0 (the "License")
 *
 * Copyright (c) 2019-2021 Megvii Inc. All rights reserved.
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT ARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */
//! 消息类型描述；图中间层的其余结构仍需迁移。
use std::any::{type_name, TypeId};

#[doc(hidden)]
#[derive(Default, Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum MsgTypeId {
    #[default]
    Any,
    Rust(TypeId),
    Python(u64),
    Template(usize),
}

impl MsgTypeId {
    pub fn of<T: 'static>() -> Self {
        Self::Rust(TypeId::of::<T>())
    }

    pub fn python(name: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let name = name.trim();
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        Self::Python(hasher.finish())
    }
}

#[derive(Clone, Debug)]
pub struct MsgType {
    pub name: String,
    pub id: MsgTypeId,
}

impl MsgType {
    pub fn any() -> Self {
        Self {
            name: "Any".to_owned(),
            id: MsgTypeId::Any,
        }
    }

    pub fn template(id: usize) -> Self {
        Self {
            name: format!("T{}", id),
            id: MsgTypeId::Template(id),
        }
    }

    pub fn of<T: 'static>() -> Self {
        Self {
            name: type_name::<T>().to_owned(),
            id: MsgTypeId::of::<T>(),
        }
    }

    pub fn python(name: &str) -> Self {
        let name = name.trim();
        Self {
            id: MsgTypeId::python(name),
            name: name.to_owned(),
        }
    }
}

/// 端口形态，与原版区分标量、数组、字典和动态端口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Unit,
    List,
    Dict,
    Dyn,
}
#[derive(Clone, Debug)]
pub struct PortInfo {
    pub name: String,
    pub ty: PortType,
    pub mty: MsgType,
}
#[derive(Clone, Debug)]
pub struct Port {
    pub node_type: String,
    pub node_name: String,
    pub port_info: PortInfo,
    pub port_tag: Option<u64>,
}
impl Port {
    /// 原版 splitn(3) 规则：第三段余下的冒号属于标签；不裁剪空白。
    pub fn parse(name: &str) -> crate::error::Result<((&str, &str), Option<u64>)> {
        let mut parts = name.splitn(3, ':');
        let pair = parts
            .next()
            .zip(parts.next())
            .ok_or_else(|| crate::error::Error::BadPortRef(name.to_owned()))?;
        Ok((pair, parts.next().map(crate::envelope::str2addr)))
    }
    pub fn is_dyn(&self) -> bool {
        matches!(self.port_info.ty, PortType::Dyn)
    }
}
