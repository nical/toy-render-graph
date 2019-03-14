pub mod misc;
pub mod texture_allocator;

pub use std::i32;
use crate::misc::*;

pub struct DeviceSpace;
pub type DeviceIntRect = euclid::TypedRect<i32, DeviceSpace>;
pub type DeviceIntPoint = euclid::TypedPoint2D<i32, DeviceSpace>;
pub type DeviceIntSize = euclid::TypedSize2D<i32, DeviceSpace>;
pub use euclid::size2;

pub use crate::misc::{AtlasAllocator, DummyAtlasAllocator, GuillotineAllocator, DbgAtlasAllocator};

#[derive(Clone)]
pub struct Node {
    pub name: String,
    pub size: DeviceIntSize,
    pub alloc_kind: AllocKind,
    pub dependencies: Vec<NodeId>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AllocKind {
    Fixed(TextureId),
    Dynamic,
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

    pub fn add_node(&mut self, name: &str, size: DeviceIntSize, alloc_kind: AllocKind, deps: &[NodeId]) -> NodeId {
        let id = node_id(self.nodes.len());
        self.nodes.push(Node {
            name: name.to_string(),
            size,
            alloc_kind,
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
    pub fixed_target: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AllocatedRect {
    pub rect: DeviceIntRect,
    pub texture: TextureId,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PassOptions {
    Linear,
    Recursive,
    Eager,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetOptions {
    Direct,
    PingPong,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuilderOptions {
    pub passes: PassOptions,
    pub targets: TargetOptions,
    pub culling: bool,
}

pub struct GraphBuilder {
    options: BuilderOptions,
}

impl GraphBuilder {
    pub fn new(options: BuilderOptions) -> Self {
        GraphBuilder { options }
    }

    pub fn build(&mut self, mut graph: Graph, allocator: &mut dyn AtlasAllocator) -> BuiltGraph {

        // Step 1 - Sort the nodes in a valid execution order (nodes appear after
        // the nodes they depend on.
        //
        // We don't have to do anything here because this property is provided
        // by construction in the Graph data structure.
        // The next step depends on this property.

        // Step 2 - Mark nodes that contribute to the result of the root nodes
        // as active.
        //
        // This pass is not strictly necessary, but, it prevents nodes that don't
        // contrbute to the result of the root nodes from being executed.

        let mut active_nodes = Vec::new();
        if self.options.culling {
            cull_nodes(&graph, &mut active_nodes);
        } else {
            active_nodes = vec![true; graph.nodes.len()];
        }

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];


        // Step 3 - Assign nodes to passes.
        //
        // The main constraint is that nodes must be in a later pass than
        // all of their dependencies.

        match self.options.passes {
            PassOptions::Eager => create_passes_eager(
                &graph.nodes,
                &mut active_nodes,
                &mut passes,
                &mut node_passes,
            ),
            PassOptions::Linear => create_passes_linear(
                &graph.nodes,
                &active_nodes,
                &mut passes,
                &mut node_passes,
            ),
            PassOptions::Recursive => create_passes_recursive(
                &graph,
                &mut active_nodes,
                &mut passes,
                &mut node_passes,
            ),
        }

        // Step 4 - assign render targets to passes.
        //
        // A render target can be used by several passes as long as no pass
        // both read and write the same render target.

        match self.options.targets {
            TargetOptions::Direct => assign_targets_direct(
                &mut passes,
                &mut graph.nodes,
                &mut node_passes,
                allocator,
            ),
            TargetOptions::PingPong => assign_targets_ping_pong(
                &mut passes,
                &mut graph.nodes,
                &mut node_passes,
                allocator,
            ),
        }

        // Step 5 - Allocate sub-rects of the render targets for each node.
        //
        // Several nodes can alias parts of a render target as long no node
        // overwite the result of a node that will be needed later.

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
/// new render passes are added each time a node is encountered that depends
/// on the last added target. As a result the number of passes is sensible to
/// the order of the nodes.
/// This method may not generate the minimal number of render passes.
///
/// This method assumes that nodes are already sorted in a valid execution order: nodes can only
/// depend on the result of nodes that appear before them in the array.
fn create_passes_linear(
    nodes: &[Node],
    active_nodes: &[bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32],
) {
    let mut current_pass_nodes = Vec::new();
    let mut prev_alloc_kind = AllocKind::Dynamic;
    for idx in 0..nodes.len() {
        if !active_nodes[idx] {
            continue;
        }

        let alloc_kind = nodes[idx].alloc_kind;
        let push_new_pass = if prev_alloc_kind != alloc_kind {
            true
        } else {
            let mut dependent_pass: i32 = -1;
            for dep in &nodes[idx].dependencies {
                dependent_pass = i32::max(dependent_pass, node_passes[dep.to_usize()]);
            }

            assert!(dependent_pass <= passes.len() as i32);

            dependent_pass == passes.len() as i32
        };

        if push_new_pass {
            let (target, fixed_target) = match prev_alloc_kind {
                AllocKind::Dynamic => (texture_id(0), false),
                AllocKind::Fixed(target) => (target, true),
            };
            passes.push(Pass {
                nodes: std::mem::replace(&mut current_pass_nodes, Vec::new()),
                target,
                fixed_target,
            });
        }

        prev_alloc_kind = alloc_kind;

        current_pass_nodes.push(node_id(idx));
        node_passes[idx] = passes.len() as i32;
    }

    if !current_pass_nodes.is_empty() {
        let (target, fixed_target) = match prev_alloc_kind {
            AllocKind::Dynamic => (texture_id(0), false),
            AllocKind::Fixed(target) => (target, true),
        };
        passes.push(Pass {
            nodes: std::mem::replace(&mut current_pass_nodes, Vec::new()),
            target,
            fixed_target,
        });
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method tries to emulate WebRender's current behavior.
/// Nodes are executed as late as possible (as opposed to create_passes_eager
/// which tends to execute nodes as early as possible).
///
/// TODO: Support fixed target allocations.
fn create_passes_recursive(
    graph: &Graph,
    active_nodes: &[bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32],
) {
    fn create_passes_recursive_impl(
        node_id: NodeId,
        rev_pass_index: usize,
        nodes: &[Node],
        node_rev_passes: &mut [usize],
        max_depth: &mut usize,
    ) {
        *max_depth = std::cmp::max(*max_depth, rev_pass_index);

        node_rev_passes[node_id.to_usize()] = std::cmp::max(
            node_rev_passes[node_id.to_usize()],
            rev_pass_index,
        );

        let node = &nodes[node_id.to_usize()];
        for &dep in &node.dependencies {
            create_passes_recursive_impl(
                dep,
                rev_pass_index + 1,
                nodes,
                node_rev_passes,
                max_depth,
            );
        }
    }

    let mut max_depth = 0;
    let mut node_rev_passes = vec![0; graph.nodes.len()];

    for &root in &graph.roots {
        create_passes_recursive_impl(
            root,
            0,
            &graph.nodes,
            &mut node_rev_passes,
            &mut max_depth,
        );
    }

    for _ in 0..(max_depth + 1) {
        passes.push(Pass {
            nodes: Vec::new(),
            target: texture_id(0),
            fixed_target: false,
        });
    }

    for node_idx in 0..graph.nodes.len() {
        if !active_nodes[node_idx] {
            continue;
        }
        let pass_index = max_depth - node_rev_passes[node_idx];
        passes[pass_index].nodes.push(node_id(node_idx));
        node_passes[node_idx] = pass_index as i32;
    }
}

/// Create render passes and assign the nodes to them.
///
/// This algorithm looks at all of the nodes, finds the ones that can be executed
/// (all dependencies are resolved in a previous pass) and adds them to the current
/// pass. When no more nodes can be added to the pass, a new pass is created and
/// we start again.
/// In practice this means nodes are executed eagerly as soon as possible even if
/// Their result is not needed in the next pass.
///
/// This method generates the minimal amount of render passes but is more expensive.
/// than create_passes_linear.
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
        let mut fixed_nodes = Vec::new();

        'pass_loop: for idx in 0..nodes.len() {
            if !active_nodes[idx] {
                continue;
            }

            let mut dependent_pass: i32 = -1;
            let node = &nodes[idx];
            for &dep in &node.dependencies {
                let pass = node_passes[dep.to_usize()];
                if pass >= current_pass {
                    continue 'pass_loop;
                }
                dependent_pass = i32::max(dependent_pass, pass);
            }

            if let AllocKind::Fixed(target) = node.alloc_kind {
                // To avoid breaking passes we record the fixed allocation nodes that are
                // ready and push them to a new pass after we are done building the current
                // one.
                fixed_nodes.push((idx, target));
            } else {
                node_passes[idx] = current_pass;
                current_pass_nodes.push(node_id(idx));
            }

            // Mark the node inactive so that we don't attempt to add it to another
            // pass.
            active_nodes[idx] = false;
        }

        let new_dynamic_nodes = !current_pass_nodes.is_empty();
        if new_dynamic_nodes {
            passes.push(Pass {
                nodes: current_pass_nodes,
                target: TextureId(current_pass as u32),
                fixed_target: false,
            });
        }

        let new_fixed_nodes = !fixed_nodes.is_empty();
        for &(idx, target) in &fixed_nodes {
            node_passes[idx] = passes.len() as i32;
            passes.push(Pass {
                nodes: vec![node_id(idx)],
                target,
                fixed_target: true,
            });
        }

        if !new_dynamic_nodes && !new_fixed_nodes {
            break;
        }
    }
}

/// Assign a render target to each pass with a "ping-pong" scheme alternating between
/// two render targets.
///
/// In order to ensure that a node never reads and writes from the same target, some
/// blit nodes may be inserted in the graph.
fn assign_targets_ping_pong(
    passes: &mut[Pass],
    nodes: &mut Vec<Node>,
    node_passes: &mut [i32],
    allocator: &mut dyn AtlasAllocator,
) {
    let mut node_redirects = vec![None; nodes.len()];

    // TODO: Figure out a strategy for target sizes.
    let texture_ids = [
        allocator.add_texture(size2(2048, 2048)),
        allocator.add_texture(size2(2048, 2048)),
    ];

    let mut last_dynamic_pass = 0;
    let mut nth_dynamic_pass = 0;
    for p in 0..passes.len() {
        if passes[p].fixed_target {
            continue;
        }
        let target = texture_ids[nth_dynamic_pass % 2];
        nth_dynamic_pass += 1;

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
                            alloc_kind: AllocKind::Dynamic,
                            size
                        });
                        node_redirects.push(None);
                        node_redirects[dep] = Some(blit_id);

                        passes[last_dynamic_pass].nodes.push(blit_id);

                        blit_id
                    };

                    nodes[n].dependencies[nth_dep] = source;
                }
            }
        }
        if !passes[p].fixed_target {
            last_dynamic_pass = p;
        }
    }
}

/// Assign a render target to each pass without adding nodes to the graph.
///
/// This method may generate more render targets than assign_targets_ping_pong,
/// however it doesn't add any extra copying operations.
fn assign_targets_direct(
    passes: &mut[Pass],
    nodes: &mut Vec<Node>,
    node_passes: &mut [i32],
    allocator: &mut dyn AtlasAllocator,
) {
    let mut targets = Vec::new();
    let mut dependency_targets = std::collections::HashSet::new();

    for p in 0..passes.len() {
        let pass = &passes[p];
        if pass.fixed_target {
            continue;
        }

        dependency_targets.clear();
        for &pass_node in &pass.nodes {
            for &dep in &nodes[pass_node.to_usize()].dependencies {
                let dep_pass = node_passes[dep.to_usize()];
                dependency_targets.insert(passes[dep_pass as usize].target);
            }
        }

        let mut target = None;
        for i in 0..targets.len() {
            let target_id = targets[i];
            if !dependency_targets.contains(&target_id) {
                target = Some(target_id);
                break;
            }
        }

        let target = target.unwrap_or_else(|| {
            let id = allocator.add_texture(size2(2048, 2048));
            targets.push(id);
            id
        });

        passes[p].target = target;
    }
}

/// Determine when is the first and last time that the sub-rect associated to the
/// result of each node is needed and allocate portions of the render targets
/// accordingly.
/// This method computes the lifetime of each node and delegates the allocation
/// logic to the AtlasAllocator implementation.
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
        if !pass.fixed_target {
            allocator.begin_pass(pass.target);
            // Allocations needed for this pass.
            for &node in &pass.nodes {
                let node_idx = node.to_usize();
                let size = graph.nodes[node_idx].size;
                allocated_rects[node_idx] = Some(AllocatedRect {
                    rect: allocator.allocate(pass.target, size),
                    texture: pass.target,
                });
            }
        }

        // Deallocations we can perform after this pass.
        let finished_range = pass_last_node_ranges[pass_index].clone();
        for finished_node in &last_node_refs[finished_range] {
            let node_idx = finished_node.to_usize();
            if let Some(allocated_rect) = allocated_rects[node_idx] {
                allocator.deallocate(
                    allocated_rect.texture,
                    &allocated_rect.rect,
                );
            }
        }
    }
}

pub fn build_and_print_graph(graph: &Graph, options: BuilderOptions, with_deallocations: bool) {
    let mut builder = GraphBuilder::new(options);
    let mut allocator = GuillotineAllocator::new();
    let mut allocator = DbgAtlasAllocator::new(&mut allocator);
    allocator.record_deallocations = with_deallocations;

    let built_graph = builder.build(graph.clone(), &mut allocator);

    let n_passes = built_graph.passes.len();
    let mut n_nodes = 0;
    for pass in &built_graph.passes {
        n_nodes += pass.nodes.len();
    }

    println!(
        "\n\n------------- deallocations: {:?}, culling: {:?}, passes: {:?}, targets: {:?}",
        with_deallocations,
        options.culling,
        options.passes,
        options.targets
    );
    println!(
        "              {:?} nodes, {:?} passes, {:?} targets, {:?} pixels (max), {:?} rects (max)",
        n_nodes,
        n_passes,
        allocator.textures.len(),
        allocator.max_allocated_pixels(),
        allocator.max_allocated_rects(),
    );

    for i in 0..built_graph.passes.len() {
        let pass = &built_graph.passes[i];
        println!("# pass {:?}  {} target {:?}",
            i,
            if pass.fixed_target { "  fixed" } else { "dynamic" },
            pass.target,
        );
        for &node in &pass.nodes {
            println!("  - {:?} {:?}      {}",
                node,
                built_graph.nodes[node.to_usize()].name,
                if let Some(r) = built_graph.allocated_rects[node.to_usize()] {
                    format!("rect: [({}, {}) {}x{}]", r.rect.origin.x, r.rect.origin.y, r.rect.size.width, r.rect.size.height)
                } else {
                    "".to_string()
                },
            );
        }
    }
}

#[test]
fn simple_graph() {
    let mut graph = Graph::new();

    let n0 = graph.add_node("useless", size2(100, 100), AllocKind::Dynamic, &[]);
    let _ = graph.add_node("useless", size2(100, 100), AllocKind::Dynamic, &[n0]);
    let n2 = graph.add_node("n2", size2(100, 100), AllocKind::Dynamic, &[]);
    let n3 = graph.add_node("n3", size2(100, 100), AllocKind::Dynamic, &[]);
    let n4 = graph.add_node("n4", size2(100, 100), AllocKind::Dynamic, &[n2, n3]);
    let n5 = graph.add_node("n5", size2(100, 100), AllocKind::Dynamic, &[]);
    let n6 = graph.add_node("n6", size2(100, 100), AllocKind::Dynamic, &[n3, n5]);
    let n7 = graph.add_node("n7", size2(100, 100), AllocKind::Dynamic, &[n2, n4, n6]);
    let n8 = graph.add_node("root", size2(800, 600), AllocKind::Fixed(TextureId(100)), &[n7]);

    graph.add_root(n5);
    graph.add_root(n8);

    for &with_deallocations in &[false, true] {
        for &pass_option in &[PassOptions::Linear, PassOptions::Recursive, PassOptions::Eager] {
            for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
                build_and_print_graph(
                    &graph,
                    BuilderOptions {
                        passes: pass_option,
                        targets: target_option,
                        culling: true,
                    },
                    with_deallocations,
                )
            }
        }
    }
}

