pub mod misc;
pub mod texture_allocator;

pub use std::i32;
use crate::misc::*;

#[derive(Clone)]
pub struct Node {
    pub name: String,
    pub size: DeviceIntSize,
    pub dependencies: Vec<NodeId>,
}

#[derive(Clone)]
pub struct Graph {
    nodes: Vec<Node>,
    roots: Vec<NodeId>,
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

pub struct BuiltGraph {
    pub nodes: Vec<Node>,
    pub roots: Vec<NodeId>,
    pub allocated_rects: Vec<Option<AllocatedRect>>,
    pub passes: Vec<Pass>,
}

pub struct Pass {
    pub nodes: Vec<NodeId>,
    pub target: TextureId,
}

#[derive(Copy, Clone,Debug, PartialEq, Eq, Hash)]
pub struct AllocatedRect {
    pub rect: DeviceIntRect,
    pub texture: TextureId,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PassGenerator {
    Simple,
    Eager,
}

pub struct GraphBuilder {
    pass_gen: PassGenerator,
}

impl GraphBuilder {
    pub fn new(pass_gen: PassGenerator) -> Self {
        GraphBuilder { pass_gen }
    }

    pub fn build(&mut self, mut graph: Graph, allocator: &mut dyn AtlasAllocator) -> BuiltGraph {
        let mut active_nodes = Vec::new();
        cull_nodes(&graph, &mut active_nodes);

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];

        match self.pass_gen {
            PassGenerator::Eager => create_passes_eager(
                &graph.nodes,
                &mut active_nodes,
                &mut passes,
                &mut node_passes,
            ),
            PassGenerator::Simple => create_passes_simple(
                &graph.nodes,
                &active_nodes,
                &mut passes,
                &mut node_passes,
            ),
        }

        assign_targets_ping_pong(
            &mut passes,
            &mut graph.nodes,
            &mut node_passes,
            allocator,
        );

        let mut allocated_rects = vec![None; graph.nodes.len()];
        reset(&mut active_nodes, graph.nodes.len(), false);
        allocate_target_rects(
            &graph,
            &passes,
            &mut active_nodes,
            &mut allocated_rects,
            allocator,
        );

        BuiltGraph {
            nodes: graph.nodes,
            roots: graph.roots,
            allocated_rects,
            passes,
        }
    }
}

fn reset<T: Copy>(vector: &mut Vec<T>, len: usize, val: T) {
    vector.clear();
    vector.reserve(len);
    for _ in 0..len {
        vector.push(val);
    }
}

/// Generate a vector containing for each node a boolean which is true if the node contributes to
/// a root of the graph.
fn cull_nodes(graph: &Graph, active_nodes: &mut Vec<bool>) {
    // Call this function recursively starting from the roots.
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

    reset(active_nodes, graph.nodes.len(), false);

    for &root in &graph.roots {
        mark_active_node(root, &graph.nodes, active_nodes);
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
    node_passes: &mut [i32],
) {

    passes.push(Pass { nodes: Vec::new(), target: TextureId(0) });

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
                target: texture_id(0),
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
            target: TextureId(current_pass as u32),
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
    node_passes: &mut [i32],
    allocator: &mut dyn AtlasAllocator,
) {
    let mut node_redirects = vec![None; nodes.len()];

    let texture_ids = [
        allocator.add_texture(size2(2048, 2048)),
        allocator.add_texture(size2(2048, 2048)),
    ];

    for p in 0..passes.len() {
        let target = texture_ids[p % 2];
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

fn allocate_target_rects(
    graph: &Graph,
    passes: &[Pass],
    visited: &mut[bool],
    allocated_rects: &mut Vec<Option<AllocatedRect>>,
    allocator: &mut dyn AtlasAllocator,
) {
    let mut last_node_refs: Vec<NodeId> = Vec::with_capacity(graph.nodes.len());
    let mut pass_last_node_ranges: Vec<std::ops::Range<usize>> = vec![0..0; passes.len()];

    // The first step is to find for each pass the list of nodes that are not referenced
    // anymore after the pass ends.

    // Mark roots as visited to avoid deallocating their target rects.
    for root in &graph.roots {
        visited[root.to_usize()] = true;
    }

    // Visit passes in reverse order and look at the dependencies.
    // Each dependency that we haven't visited yet is the last reference to a node.
    let mut pass_index = passes.len();
    for pass in passes.iter().rev() {
        pass_index -= 1;
        let first = last_node_refs.len();
        for &n in &pass.nodes {
            for &dep in &graph.nodes[n.to_usize()].dependencies {
                let dep_idx = dep.to_usize();
                if !visited[dep_idx] {
                    visited[dep_idx] = true;
                    last_node_refs.push(dep);
                }
            }
        }

        pass_last_node_ranges[pass_index] = first..last_node_refs.len();
    }

    // In the second step we go through each pass in order and perform allocations/deallocations.
    for (pass_index, pass) in passes.iter().enumerate() {
        allocator.flush_deallocations(pass.target);
        // Allocations needed for this pass.
        for &node in &pass.nodes {
            let node_idx = node.to_usize();
            let size = graph.nodes[node_idx].size;
            allocated_rects[node_idx] = Some(AllocatedRect {
                rect: allocator.allocate(pass.target, size),
                texture: pass.target,
            });
        }

        // Deallocations we can perform after this pass.
        let finished_range = pass_last_node_ranges[pass_index].clone();
        for finished_node in &last_node_refs[finished_range] {
            let node_idx = finished_node.to_usize();
            let allocated_rect = allocated_rects[node_idx].unwrap();
            allocator.deallocate(
                allocated_rect.texture,
                &allocated_rect.rect,
            );
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

    for &pass_generator in &[PassGenerator::Simple, PassGenerator::Eager] {
        let mut builder = GraphBuilder::new(pass_generator);
        let built_graph = builder.build(graph.clone(), &mut GuillotineAllocator::new());

        println!("\n------------- {:?}", pass_generator);
        for i in 0..built_graph.passes.len() {
            println!("# pass {:?} target {:?}", i, built_graph.passes[i].target);
            for &node in &built_graph.passes[i].nodes {
                println!("- node {:?} ({:?})", built_graph.nodes[node.to_usize()].name, node);
            }
        }
    }
}

