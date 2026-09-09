//! 静态压平图的模板推导；以索引和独占借用替代原版 InferCtx 的裸指针修改。
use super::{interlayer::MsgTypeId, GraphConfig, PortRef};
use crate::{
    channel::guess_channel_type,
    error::{Error, Result},
    registry,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Port {
    node: String,
    name: String,
    input: bool,
    ty: MsgTypeId,
}
struct Edge {
    tx: Vec<usize>,
    rx: Vec<usize>,
}
#[derive(Clone, Copy)]
enum State {
    Pending,
    Visiting,
    Done(MsgTypeId),
}
struct Infer {
    ports: Vec<Port>,
    edges: Vec<Edge>,
    state: Vec<State>,
}
impl Infer {
    fn visit(&mut self, edge: usize) -> Result<()> {
        if !matches!(self.state[edge], State::Pending) {
            return Ok(());
        }
        self.state[edge] = State::Visiting;
        let endpoints: Vec<_> = self.edges[edge]
            .tx
            .iter()
            .chain(&self.edges[edge].rx)
            .copied()
            .collect();
        for &port in &endpoints {
            if let MsgTypeId::Template(id) = self.ports[port].ty {
                let associated: Vec<_> = self
                    .ports
                    .iter()
                    .enumerate()
                    .filter(|(index, other)| {
                        *index != port
                            && other.node == self.ports[port].node
                            && other.ty == MsgTypeId::Template(id)
                    })
                    .map(|(index, _)| index)
                    .collect();
                'related: for other in associated {
                    let connected: Vec<_> = self
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.tx.contains(&other) || e.rx.contains(&other))
                        .map(|(index, _)| index)
                        .collect();
                    for next in connected {
                        if self.visit(next).is_ok() {
                            break 'related;
                        }
                    }
                }
            }
        }
        self.state[edge] = State::Pending;
        let tx: HashSet<_> = self.edges[edge]
            .tx
            .iter()
            .map(|p| self.ports[*p].ty)
            .collect();
        let rx: HashSet<_> = self.edges[edge]
            .rx
            .iter()
            .map(|p| self.ports[*p].ty)
            .collect();
        let selected = guess_channel_type(&tx, &rx)?;
        self.state[edge] = State::Done(selected);
        for port in endpoints {
            if let MsgTypeId::Template(id) = self.ports[port].ty {
                let node = self.ports[port].node.clone();
                for p in &mut self.ports {
                    if p.node == node && p.ty == MsgTypeId::Template(id) {
                        p.ty = selected;
                    }
                }
            }
        }
        Ok(())
    }
}

/// 返回顺序：图输入、图输出、内部连接，与 Builder 分配队列的顺序一致。
pub(crate) struct InferredGraph {
    pub connections: Vec<MsgTypeId>,
    ports: HashMap<(String, String, bool), MsgTypeId>,
}
impl InferredGraph {
    pub fn port_type(&self, node: &str, port: &str, input: bool) -> MsgTypeId {
        self.ports[&(node.to_owned(), port.to_owned(), input)]
    }
}
pub(crate) fn infer(g: &GraphConfig) -> Result<InferredGraph> {
    let mut ports = Vec::new();
    let mut lookup = HashMap::new();
    for node in &g.nodes {
        let reg =
            registry::find(&node.ty).ok_or_else(|| Error::UnknownNodeType(node.ty.clone()))?;
        for (input, names, types) in [
            (true, reg.inputs, (reg.input_types)()),
            (false, reg.outputs, (reg.output_types)()),
        ] {
            if names.len() != types.len() {
                return Err(Error::BadConnection(
                    "port type metadata length does not match names".into(),
                ));
            }
            for (name, ty) in names.iter().zip(types) {
                lookup.insert((node.name.as_str(), *name, input), ports.len());
                ports.push(Port {
                    node: node.name.clone(),
                    name: (*name).into(),
                    input,
                    ty,
                });
            }
        }
    }
    let resolve = |reference: &str, direction: Option<bool>| -> Result<usize> {
        let p = PortRef::parse(reference)?;
        if !g.nodes.iter().any(|n| n.name == p.node) {
            return Err(Error::UnknownNode(p.node.into()));
        }
        direction
            .map(|input| lookup.get(&(p.node, p.port, input)).copied())
            .unwrap_or_else(|| {
                lookup
                    .get(&(p.node, p.port, false))
                    .or_else(|| lookup.get(&(p.node, p.port, true)))
                    .copied()
            })
            .ok_or_else(|| Error::UnknownPort {
                node: p.node.into(),
                port: p.port.into(),
            })
    };
    let mut edges = Vec::new();
    for boundary in &g.inputs {
        if boundary.ports.is_empty() {
            return Err(Error::BadConnection("graph input has no target".into()));
        }
        edges.push(Edge {
            tx: vec![],
            rx: boundary
                .ports
                .iter()
                .map(|p| resolve(p, Some(true)))
                .collect::<Result<_>>()?,
        });
    }
    for boundary in &g.outputs {
        if boundary.ports.is_empty() {
            return Err(Error::BadConnection("graph output has no source".into()));
        }
        edges.push(Edge {
            rx: vec![],
            tx: boundary
                .ports
                .iter()
                .map(|p| resolve(p, Some(false)))
                .collect::<Result<_>>()?,
        });
    }
    for connection in &g.connections {
        let mut edge = Edge {
            tx: vec![],
            rx: vec![],
        };
        for reference in &connection.ports {
            let index = resolve(reference, None)?;
            if ports[index].input {
                edge.rx.push(index);
            } else {
                edge.tx.push(index);
            }
        }
        if edge.tx.is_empty() || edge.rx.is_empty() {
            return Err(Error::BadConnection(
                "connection needs sender and receiver".into(),
            ));
        }
        edges.push(edge);
    }
    let mut infer = Infer {
        state: vec![State::Pending; edges.len()],
        ports,
        edges,
    };
    for edge in 0..infer.edges.len() {
        if let Err(error) = infer.visit(edge) {
            if matches!(error, Error::TemplateInferFault) {
                // 原版 infer_graph 的回退只记录此连接，不把整个节点模板强制绑定为 Any。
                infer.state[edge] = State::Done(MsgTypeId::Any);
            } else {
                return Err(error);
            }
        }
    }
    let connections = infer
        .state
        .into_iter()
        .map(|state| match state {
            State::Done(ty) => ty,
            _ => unreachable!("inference left an unfinished connection"),
        })
        .collect();
    let ports = infer
        .ports
        .into_iter()
        .map(|p| ((p.node, p.name, p.input), p.ty))
        .collect();
    Ok(InferredGraph { connections, ports })
}
