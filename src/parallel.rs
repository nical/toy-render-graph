use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc::{channel, Sender, Receiver},
};
use euclid::{size2};
use smallvec::SmallVec;
use crate::graph::*;

enum Edit {
    AddNode(Node, NodeId),
    AddDependency(NodeId, NodeId),
    AddRoot(NodeId),
}

#[derive(Clone)]
pub struct ParallelGraphBuilder {
    next_node_id: Arc<AtomicUsize>,
    sender: Sender<Edit>,
}

impl ParallelGraphBuilder {
    pub fn add_node(
        &self,
        task_id: TaskId,
        target_kind: TargetKind,
        size: Size,
        alloc_kind: AllocKind,
        deps: &[NodeId]
    ) -> NodeId {
        let id = node_id(self.next_node_id.fetch_add(1, Ordering::SeqCst));
        self.sender.send(Edit::AddNode(
            Node {
                task_id,
                size,
                alloc_kind,
                dependencies: SmallVec::from_slice(deps),
                target_kind,
            },
            id,
        )).unwrap();

        id
    }

    pub fn add_dependency(&self, node: NodeId, dep: NodeId) {
        self.sender.send(Edit::AddDependency(node, dep)).unwrap();
    }

    pub fn add_root(&self, node: NodeId) {
        self.sender.send(Edit::AddRoot(node)).unwrap();
    }
}


pub struct ParallelGraphReceiver {
    next_node_id: Arc<AtomicUsize>,
    sender: Sender<Edit>,
    receiver: Receiver<Edit>,
}

impl ParallelGraphReceiver {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        ParallelGraphReceiver {
            next_node_id: Arc::new(AtomicUsize::new(0)),
            sender,
            receiver,
        }
    }

    pub fn new_builder(&self) -> ParallelGraphBuilder {
        ParallelGraphBuilder {
            next_node_id: self.next_node_id.clone(),
            sender: self.sender.clone(),
        }
    }

    pub fn resolve(&self) -> Graph {
        let capacity = self.next_node_id.load(Ordering::SeqCst) - 1;
        self.next_node_id.store(0, Ordering::SeqCst);

        let mut graph = Graph::with_capacity(capacity, 0);

        loop {
            match self.receiver.try_recv() {
                Ok(Edit::AddNode(node, id)) => {
                    // Push a dummy nodes if the index doesn't exist yet.
                    while id.index() > graph.nodes.len() {
                        graph.nodes.push(Node {
                            task_id: TaskId::Render(std::u16::MAX, std::u32::MAX),
                            size: size2(0, 0),
                            alloc_kind: AllocKind::Dynamic,
                            dependencies: SmallVec::new(),
                            target_kind: TargetKind::Color,
                        });
                    }

                    if id.index() == graph.nodes.len() {
                        // Common case.
                        graph.nodes.push(node);
                    } else {
                        graph.nodes[id.index()] = node;
                    }
                }
                Ok(Edit::AddDependency(node, dep)) => {
                    graph.nodes[node.index()].dependencies.push(dep);
                }
                Ok(Edit::AddRoot(id)) => {
                    graph.roots.push(id);
                }
                Err(..) => {
                    break;
                }
            }
        }

        graph
    }
}
