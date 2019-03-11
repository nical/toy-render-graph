use std::usize;
use std::i32;

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DeviceIntSize {
    pub x: i32,
    pub y: i32,
}

pub fn size2(x: i32, y: i32) -> DeviceIntSize {
    DeviceIntSize { x, y }
}

#[derive(Clone)]
pub struct Node {
    name: String,
    size: DeviceIntSize,
    dependencies: Vec<NodeId>,
}

#[derive(Clone)]
pub struct Graph {
    nodes: Vec<Node>,
    roots: Vec<NodeId>,
}

pub struct BuiltGraph {
    pub passes: Vec<Pass>,
}

pub struct Pass {
    pub nodes: Vec<NodeId>,
    pub target: u32,
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            nodes: Vec::new(),
            roots: Vec::new(),
        }
    }

    pub fn add_node(&mut self, name: &str, size: DeviceIntSize, deps: &[NodeId]) -> NodeId {
        let id = node_id(self.nodes.len());
        self.nodes.push(Node {
            name: name.to_string(),
            size,
            dependencies: deps.to_vec(),
        });

        id
    }

    pub fn add_root(&mut self, id: NodeId) {
        self.roots.push(id);
    }
}

pub struct GraphBuilder {
    
}

impl GraphBuilder {
    pub fn new() -> Self {
        GraphBuilder {}
    }

    pub fn build(&mut self, graph: &mut Graph) -> BuiltGraph {
        let mut active_nodes = Vec::new();
        cull_nodes(&graph, &mut active_nodes);

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];

        create_passes_eager(
            &graph.nodes,
            &mut active_nodes,
            &mut passes,
            &mut node_passes,
        );

        assign_targets_ping_pong(
            &mut passes,
            &mut graph.nodes,
            &mut node_passes,
        );

        BuiltGraph {
            passes,
        }
    }
}

pub struct GraphBuilderSimple {
    
}

impl GraphBuilderSimple {
    pub fn new() -> Self {
        GraphBuilderSimple {}
    }

    pub fn build(&mut self, graph: &mut Graph) -> BuiltGraph {
        let mut active_nodes = Vec::new();
        cull_nodes(&graph, &mut active_nodes);

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];

        create_passes_simple(
            &graph.nodes,
            &active_nodes,
            &mut passes,
            &mut node_passes,
        );

        assign_targets_ping_pong(
            &mut passes,
            &mut graph.nodes,
            &mut node_passes,
        );

        BuiltGraph {
            passes,
        }
    }
}


/// Generate a vector containing for each node a boolean which is true if the node contributes to
/// a root of the graph.
fn cull_nodes(graph: &Graph, active_nodes: &mut Vec<bool>) {
    active_nodes.clear();
    active_nodes.reserve(graph.nodes.len());
    for _ in 0..graph.nodes.len() {
        active_nodes.push(false);
    }
    for &root in &graph.roots {
        mark_active_node(root, &graph.nodes, active_nodes);
    }
}

fn mark_active_node(id: NodeId, nodes: &[Node], active_nodes: &mut [bool]) {
    let idx = id.to_usize();
    if active_nodes[idx] {
        return;
    }

    active_nodes[idx] = true;

    for &dep in &nodes[idx].dependencies {
        mark_active_node(dep, nodes, active_nodes);
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method may not generate the minimal number of render passes.
///
/// This method assumes that nodes are already sorted in a valid execution order: nodes can only
/// depend on the result of nodes that appear before them in the array.
fn create_passes_simple(
    nodes: &[Node],
    active_nodes: &[bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32]
) {

    passes.push(Pass { nodes: Vec::new(), target: 0 });

    for idx in 0..nodes.len() {
        if !active_nodes[idx] {
            continue;
        }

        let mut dependent_pass: i32 = -1;
        for dep in &nodes[idx].dependencies {
            dependent_pass = i32::max(dependent_pass, node_passes[dep.to_usize()]);
        }

        assert!(dependent_pass < passes.len() as i32);
        
        if dependent_pass == passes.len() as i32 - 1 {
            passes.push(Pass {
                nodes: Vec::new(),
                target: passes.len() as u32,
            });
        }

        node_passes[idx] = passes.len() as i32 - 1;
        passes.last_mut().unwrap().nodes.push(node_id(idx));
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method generates the minimal amount of render passes.
///
/// This method assumes that nodes are already sorted in a valid execution order: nodes can only
/// depend on the result of nodes that appear before them in the array.
fn create_passes_eager(
    nodes: &[Node],
    active_nodes: &mut [bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32]
) {
    loop {
        let current_pass = passes.len() as i32;
        let mut current_pass_nodes = Vec::new();

        'pass_loop: for idx in 0..nodes.len() {
            if !active_nodes[idx] {
                continue;
            }

            let mut dependent_pass: i32 = -1;
            for &dep in &nodes[idx].dependencies {
                let pass = node_passes[dep.to_usize()];
                if pass >= current_pass {
                    continue 'pass_loop;
                }
                dependent_pass = i32::max(dependent_pass, pass);
            }

            // Mark the node inactive so that we don't attempt to add it to another
            // pass. 
            active_nodes[idx] = false;
            node_passes[idx] = current_pass;
            current_pass_nodes.push(node_id(idx));
        }

        if current_pass_nodes.is_empty() {
            break;
        }

        passes.push(Pass {
            nodes: current_pass_nodes,
            target: current_pass as u32,
        });
    }
}

/// Assign a render target to each pass with a "ping-pong" scheme using only two render targets.
///
/// In order to ensure that a node never reads and writes from the same target, some blit nodes
/// may be inserted in the graph.
fn assign_targets_ping_pong(
    passes: &mut[Pass],
    nodes: &mut Vec<Node>,
    node_passes: &mut [i32]
) {
    let mut node_redirects = vec![None; nodes.len()];

    for p in 0..passes.len() {
        let target = p as u32 % 2;
        passes[p].target = target;
        for nth_node in 0..passes[p].nodes.len() {
            let n = passes[p].nodes[nth_node].to_usize();
            for nth_dep in 0..nodes[n].dependencies.len() {
                let dep = nodes[n].dependencies[nth_dep].to_usize();
                let dep_pass = node_passes[dep] as usize;

                // Can't both read and write the same target.
                if passes[dep_pass].target == target {
                    
                    // See if we have already added a blit task to avoid the problem.
                    let source = if let Some(source) = node_redirects[dep] {
                        source
                    } else {
                        // Otherwise add a blit task.
                        let blit_id = node_id(nodes.len());
                        let size = nodes[dep].size;
                        nodes.push(Node {
                            name: format!("blit({:?})", dep),
                            dependencies: vec![node_id(dep)],
                            size
                        });
                        node_redirects.push(None);
                        node_redirects[dep] = Some(blit_id);

                        passes[p - 1].nodes.push(blit_id);
                        
                        blit_id
                    };

                    nodes[n].dependencies[nth_dep] = source;
                }
            }
        }
    }
}

#[test]
fn it_works() {
    let mut graph = Graph::new();

    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n6 = graph.add_node("n6", size2(100, 100), &[n1, n2, n3, n4, n5]);
    let n7 = graph.add_node("root", size2(800, 600), &[n6]);

    graph.add_root(n7);
    graph.add_root(n4);

    let mut graph_1 = graph.clone();

    let mut builder = GraphBuilderSimple::new();
    let built = builder.build(&mut graph_1);
    
    println!("\n------------- simple pass generation");
    for i in 0..built.passes.len() {
        println!("# pass {:?} target {:?}", i, built.passes[i].target);
        for &node in &built.passes[i].nodes {
            println!("- node {:?} ({:?})", graph_1.nodes[node.to_usize()].name, node);
        }
    }

    let mut graph_2 = graph.clone();

    let mut builder = GraphBuilder::new();
    let built = builder.build(&mut graph_2);
    
    println!("\n------------- eager pass generation");
    for i in 0..built.passes.len() {
        println!("# pass {:?} target {:?}", i, built.passes[i].target);
        for &node in &built.passes[i].nodes {
            println!("- node {:?} ({:?})", graph_2.nodes[node.to_usize()].name, node);
        }
    }
}

