
use std::i32;
use smallvec::SmallVec;

pub use guillotiere::{Rectangle, Size, Point};
pub use euclid::{size2, vec2, point2};

pub use crate::allocator::{TextureId, TextureAllocator, GuillotineAllocator, DbgTextureAllocator};

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn index(self) -> usize { self.0 as usize }
}

pub(crate) fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NodeIdRange {
    start: u32,
    end: u32,
}

impl Iterator for NodeIdRange {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.start >= self.end {
            return None;
        }

        let result = Some(NodeId(self.start));
        self.start += 1;

        result
    }
}

impl NodeIdRange {
    #[inline]
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    #[inline]
    pub fn get(&self, nth: usize) -> NodeId {
        assert!(nth < self.len());
        NodeId(self.start + nth as u32)
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Color = 0,
    Alpha = 1,
}

const NUM_TARGET_KINDS: usize = 2;

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Node {
    pub task_id: TaskId,
    pub size: Size,
    pub alloc_kind: AllocKind,
    pub dependencies: SmallVec<[NodeId; 2]>,
    pub target_kind: TargetKind,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AllocKind {
    Fixed(TextureId, Point),
    Dynamic,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TaskId {
    Copy,
    Render(u16, u32),
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Graph {
    pub(crate) nodes: Vec<Node>,
    pub(crate) roots: Vec<NodeId>,
}

impl Graph {
    pub fn with_capacity(nodes: usize, roots: usize) -> Self {
        Graph {
            nodes: Vec::with_capacity(nodes),
            roots: Vec::with_capacity(roots),
        }
    }

    pub fn new() -> Self {
        Graph {
            nodes: Vec::new(),
            roots: Vec::new(),
        }
    }

    pub fn add_node(&mut self, task_id: TaskId, target_kind: TargetKind, size: Size, alloc_kind: AllocKind, deps: &[NodeId]) -> NodeId {
        let id = node_id(self.nodes.len());
        self.nodes.push(Node {
            task_id,
            size,
            alloc_kind,
            dependencies: SmallVec::from_slice(deps),
            target_kind,
        });

        id
    }

    pub fn add_dependency(&mut self, node: NodeId, dep: NodeId) {
        self.nodes[node.index()].dependencies.push(dep);
    }

    pub fn add_root(&mut self, id: NodeId) {
        self.roots.push(id);
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    pub fn node_ids(&self) -> NodeIdRange {
        NodeIdRange {
            start: 0,
            end: self.nodes.len() as u32,
        }
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_dependencies(&self, node: NodeId) -> &[NodeId] {
        &self.nodes[node.index()].dependencies
    }
}

impl std::ops::Index<NodeId> for Graph {
    type Output = Node;
    fn index(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct BuiltGraph {
    graph: Graph,
    allocated_rectangles: Vec<Rectangle>,
    passes: Vec<Pass>,
}

impl BuiltGraph {
    pub fn allocated_rectangle(&self, node: NodeId) -> &Rectangle {
        &self.allocated_rectangles[node.index()]
    }

    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }
}

impl std::ops::Deref for BuiltGraph {
    type Target = Graph;
    fn deref(&self) -> &Graph {
        &self.graph
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct Pass {
    pub dynamic_targets: [PassTarget; NUM_TARGET_KINDS],
    pub fixed_targets: Vec<PassTarget>,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Task {
    pub node_id: NodeId,
    pub task_id: TaskId,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct PassTarget {
    pub(crate) tasks: Vec<Task>,
    pub(crate) destination: Option<TextureId>,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetDestination {
    Dynamic(TextureId),
    Fixed(TextureId),
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetOptions {
    Direct,
    PingPong,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuilderOptions {
    pub targets: TargetOptions,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct GraphBuilder {
    options: BuilderOptions,
}

impl GraphBuilder {
    pub fn new(options: BuilderOptions) -> Self {
        GraphBuilder { options }
    }

    pub fn build(&mut self, mut graph: Graph, allocator: &mut dyn TextureAllocator) -> BuiltGraph {

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];


        // Step 1 - Assign nodes to passes.
        //
        // The main constraint is that nodes must be in a later pass than
        // all of their dependencies.

        create_passes(
            &graph,
            &mut passes,
            &mut node_passes,
        );

        // Step 2 - assign render targets to passes.
        //
        // A render target can be used by several passes as long as no pass
        // both read and write the same render target.

        match self.options.targets {
            TargetOptions::Direct => assign_targets_direct(
                &mut graph,
                &mut passes,
                &mut node_passes,
                allocator,
            ),
            TargetOptions::PingPong => assign_targets_ping_pong(
                &mut graph,
                &mut passes,
                &mut node_passes,
                allocator,
            ),
        }

        // Step 3 - Allocate portions of the render targets for each node.
        //
        // Several nodes can alias parts of a render target as long no node
        // overwrite the result of a node that will be needed later.

        let mut allocated_rectangles = vec![Rectangle::zero(); graph.nodes.len()];

        allocate_target_rects(
            &graph,
            &mut passes,
            &mut allocated_rectangles,
            allocator,
        );

        BuiltGraph {
            graph: graph,
            allocated_rectangles,
            passes,
        }
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method tries to emulate WebRender's current behavior.
/// Nodes are executed as late as possible.
fn create_passes(
    graph: &Graph,
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32],
) {
    // Recursively traverse the graph from the roots and assign a "depth" to each node.
    // The depth of a node is its maximum distance to a root, used to decide which pass
    // each node gets assigned to by simply computing `node_pass = max_depth - node_depth`.
    // This scheme ensures that nodes are executed in passes prior to nodes that depend
    // on them.

    fn assign_depths(
        graph: &Graph,
        node_id: NodeId,
        rev_pass_index: i32,
        node_rev_passes: &mut [i32],
        max_depth: &mut i32,
    ) {
        *max_depth = std::cmp::max(*max_depth, rev_pass_index);

        node_rev_passes[node_id.index()] = std::cmp::max(
            node_rev_passes[node_id.index()],
            rev_pass_index,
        );

        for &dep in &graph.nodes[node_id.index()].dependencies {
            assign_depths(
                graph,
                dep,
                rev_pass_index + 1,
                node_rev_passes,
                max_depth,
            );
        }
    }

    // Initialize the array with negative values. Once the recusive passes are done, any negative
    // value left corresponds to nodes that haven't been traversed, which means they are not
    // contributing to the output of the graph. They won't be assigned to any pass.
    let mut node_rev_passes = vec![-1; graph.nodes.len()];
    let mut max_depth = 0;

    for &root in &graph.roots {
        assign_depths(
            &graph,
            root,
            0,
            &mut node_rev_passes,
            &mut max_depth,
        );
    }

    for _ in 0..(max_depth + 1) {
        passes.push(Pass {
            dynamic_targets: [
                PassTarget {
                    tasks: Vec::new(),
                    destination: None,
                },
                PassTarget {
                    tasks: Vec::new(),
                    destination: None,
                },
            ],
            fixed_targets: Vec::new(),
        });
    }

    for id in graph.node_ids() {
        let node_idx = id.index();
        if node_rev_passes[node_idx] < 0 {
            // This node does not contribute to the output of the graph.
            continue;
        }

        let target_kind = graph.nodes[node_idx].target_kind;
        let pass_index = (max_depth - node_rev_passes[node_idx]) as usize;
        let node = &graph.nodes[node_idx];
        match graph.nodes[node_idx].alloc_kind {
            AllocKind::Dynamic => {
                passes[pass_index].dynamic_targets[target_kind as usize].tasks.push(Task {
                    node_id: id,
                    task_id: node.task_id,
                });
            }
            AllocKind::Fixed(texture_id, ..) => {
                let task = Task {
                    node_id: id,
                    task_id: node.task_id,
                };
                let mut new_target = true;
                for target in &mut passes[pass_index].fixed_targets {
                    if target.destination == Some(texture_id) {
                        new_target = false;
                        target.tasks.push(task);
                        break;
                    }
                }
                if new_target {
                    passes[pass_index].fixed_targets.push(PassTarget {
                        tasks: vec![task],
                        destination: Some(texture_id),
                    });
                }
            }
        }
        node_passes[node_idx] = pass_index as i32;
    }
}

/// Assign a render target to each pass with a "ping-pong" scheme alternating between
/// two render targets.
///
/// In order to ensure that a node never reads and writes from the same target, some
/// copy nodes may be inserted in the graph.
fn assign_targets_ping_pong(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut node_redirects = vec![None; graph.nodes.len()];

    let texture_ids = [
        // color
        [
            allocator.add_texture(),
            allocator.add_texture(),
        ],
        // alpha
        [
            allocator.add_texture(),
            allocator.add_texture(),
        ],
    ];

    let mut nth_dynamic_pass = [
        // color
        0,
        // alpha
        0,
    ];

    for p in 0..passes.len() {
        for target_kind_index in 0..NUM_TARGET_KINDS {
            if passes[p].dynamic_targets[target_kind_index].tasks.is_empty() {
                continue;
            }

            let ping_pong = nth_dynamic_pass[target_kind_index] % 2;
            nth_dynamic_pass[target_kind_index] += 1;

            let current_destination = texture_ids[target_kind_index][ping_pong];
            passes[p].dynamic_targets[target_kind_index].destination = Some(current_destination);

            for nth_node in 0..passes[p].dynamic_targets[target_kind_index].tasks.len() {
                let node = passes[p].dynamic_targets[target_kind_index].tasks[nth_node].node_id;
                for dep_idx in 0..graph.nodes[node.index()].dependencies.len() {
                    let dep = graph.nodes[node.index()].dependencies[dep_idx];
                    let dep_pass = node_passes[dep.index()] as usize;
                    let dep_target_kind = graph.nodes[dep.index()].target_kind;

                    // Can't both read and write the same target.
                    if passes[dep_pass].dynamic_targets[dep_target_kind as usize].destination == Some(current_destination) {
                        graph.nodes[node.index()].dependencies[dep_idx] = handle_conflict_using_copy_task(
                            graph,
                            passes,
                            &mut node_redirects,
                            dep,
                            dep_target_kind,
                            p
                        );
                    }
                }
            }
        }
    }
}

fn handle_conflict_using_copy_task(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_redirects: &mut[Option<NodeId>],
    dep: NodeId,
    dep_target_kind: TargetKind,
    pass: usize,
) -> NodeId {
    // See if we have already added a copy task to avoid the problem.
    if let Some(source) = node_redirects[dep.index()] {
        return source;
    }

    // Otherwise add a copy task.
    let copy_id = node_id(graph.nodes.len());
    let size = graph.nodes[dep.index()].size;
    let target_kind = graph.nodes[dep.index()].target_kind;
    graph.nodes.push(Node {
        task_id: TaskId::Copy,
        dependencies: smallvec![dep],
        alloc_kind: AllocKind::Dynamic,
        size,
        target_kind,
    });
    node_redirects[dep.index()] = Some(copy_id);

    passes[pass - 1]
        .dynamic_targets[dep_target_kind as usize]
        .tasks
        .push(Task {
            node_id: copy_id,
            task_id: TaskId::Copy,
        });

    copy_id
}

/// Assign a render target to each pass without adding nodes to the graph.
///
/// This method may generate more render targets than assign_targets_ping_pong,
/// however it doesn't add any extra copying operations.
fn assign_targets_direct(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut allocated_textures = [Vec::new(), Vec::new()];
    let mut dependencies = std::collections::HashSet::new();

    for p in 0..passes.len() {
        let pass = &passes[p];

        dependencies.clear();
        for target in &pass.dynamic_targets {
            for task in &target.tasks {
                for &dep in graph.node_dependencies(task.node_id) {
                    let dep_pass = node_passes[dep.index()];
                    let target_kind = graph.nodes[dep.index()].target_kind;
                    if let Some(id) = passes[dep_pass as usize].dynamic_targets[target_kind as usize].destination {
                        dependencies.insert(id);
                    }
                }
            }
        }

        for target_kind_index in 0..NUM_TARGET_KINDS {
            if passes[p].dynamic_targets[target_kind_index].tasks.is_empty() {
                continue;
            }

            let mut destination = None;
            for target_id in &allocated_textures[target_kind_index] {
                if !dependencies.contains(target_id) {
                    destination = Some(*target_id);
                    break;
                }
            }

            let destination = destination.unwrap_or_else(|| {
                let id = allocator.add_texture();
                allocated_textures[target_kind_index].push(id);
                id
            });

            passes[p].dynamic_targets[target_kind_index].destination = Some(destination);
        }
    }
}

/// Determine when is the first and last time that the sub-rect associated to the
/// result of each node is needed and allocate portions of the render targets
/// accordingly.
/// This method computes the lifetime of each node and delegates the allocation
/// logic to the TextureAllocator implementation.
fn allocate_target_rects(
    graph: &Graph,
    passes: &[Pass],
    allocated_rectangles: &mut[Rectangle],
    allocator: &mut dyn TextureAllocator,
) {
    // The allocation ids we get from the texture allocator.
    let mut alloc_ids = vec![None; graph.nodes.len()];

    let mut visited = vec![false; graph.nodes.len()];
    let mut last_node_refs: Vec<NodeId> = Vec::with_capacity(graph.nodes.len());
    let mut pass_last_node_ranges: Vec<std::ops::Range<usize>> = vec![0..0; passes.len()];

    // The first step is to find for each pass the list of nodes that are not referenced
    // anymore after the pass ends.

    // Mark roots as visited to avoid deallocating their target rects.
    for root in &graph.roots {
        visited[root.index()] = true;
    }

    // Visit passes in reverse order and look at the dependencies.
    // Each dependency that we haven't visited yet is the last reference to a node.
    let mut pass_index = passes.len();
    for pass in passes.iter().rev() {
        pass_index -= 1;
        let first = last_node_refs.len();
        for target_kind in 0..NUM_TARGET_KINDS {
            for task in &pass.dynamic_targets[target_kind].tasks {
                for &dep in graph.node_dependencies(task.node_id) {
                    let dep_idx = dep.index();
                    if !visited[dep_idx] {
                        visited[dep_idx] = true;
                        last_node_refs.push(dep);
                    }
                }
            }
        }
        pass_last_node_ranges[pass_index] = first..last_node_refs.len();
    }

    // In the second step we go through each pass in order and perform allocations/deallocations.
    for (pass_index, pass) in passes.iter().enumerate() {
        for pass_target in &pass.dynamic_targets {
            if pass_target.tasks.is_empty() {
                continue;
            }
            let texture = pass_target.destination.unwrap();

            // Allocations needed for this pass.
            for task in &pass_target.tasks {
                let node_idx = task.node_id.index();
                let node = &graph.nodes[node_idx];
                let size = graph.nodes[node_idx].size;
                allocated_rectangles[node_idx] = match node.alloc_kind {
                    AllocKind::Dynamic => {
                        let alloc = allocator.allocate(texture, size);
                        alloc_ids[node_idx] = Some(alloc.id);
                        alloc.rectangle
                    }
                    AllocKind::Fixed(_, origin) => Rectangle {
                        min: origin,
                        max: origin + size.to_vector(),
                    }
                };
            }
        }

        // Deallocations we can perform after this pass.
        let finished_range = pass_last_node_ranges[pass_index].clone();
        for finished_node in &last_node_refs[finished_range] {
            let node_idx = finished_node.index();
            if let Some(alloc_id) = alloc_ids[node_idx] {
                allocator.deallocate(alloc_id);
            }
        }
    }
}

pub fn build_and_print_graph(graph: &Graph, options: BuilderOptions, with_deallocations: bool) {
    let mut builder = GraphBuilder::new(options);
    let mut allocator = GuillotineAllocator::new(size2(1024, 1024));
    let mut allocator = DbgTextureAllocator::new(&mut allocator);
    allocator.record_deallocations = with_deallocations;

    let built_graph = builder.build(graph.clone(), &mut allocator);

    let n_passes = built_graph.passes.len();
    let mut n_nodes = 0;
    for pass in &built_graph.passes {
        for target in &pass.dynamic_targets {
            n_nodes += target.tasks.len();
        }
        for target in &pass.fixed_targets {
            n_nodes += target.tasks.len();
        }
    }

    println!(
        "\n\n------------- deallocations: {:?}, targets: {:?}",
        with_deallocations,
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
        println!("# pass {:?}", i);
        for &target_kind in &[TargetKind::Color, TargetKind::Alpha] {
            if let Some(texture) = pass.dynamic_targets[target_kind as usize].destination {
                println!("  * Dynamic {:?} target {:?}:", target_kind, texture);
                for task in &pass.dynamic_targets[target_kind as usize].tasks {
                    let r = built_graph.allocated_rectangle(task.node_id);
                    println!("     - {:?} {:?}      rect: [({}, {}) {}x{}]",
                        task.node_id,
                        built_graph.nodes[task.node_id.index()].task_id,
                        r.min.x, r.min.y, r.size().width, r.size().height,
                    );
                }
            }
        }

        for target in &pass.fixed_targets {
            println!("  * Fixed target {:?}:", target.destination.unwrap());
            for task in &target.tasks {
                let r = built_graph.allocated_rectangle(task.node_id);
                println!("     - {:?} {:?}      rect: [({}, {}) {}x{}]",
                    task.node_id,
                    built_graph.nodes[task.node_id.index()].task_id,
                    r.min.x, r.min.y, r.size().width, r.size().height,
                );
            }
        }
    }
}

#[test]
fn simple_graph() {
    let mut graph = Graph::new();

    let n0 = graph.add_node(TaskId::Render(0, 0), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let _ = graph.add_node(TaskId::Render(0, 1), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n0]);
    let n2 = graph.add_node(TaskId::Render(0, 2), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n3 = graph.add_node(TaskId::Render(0, 3), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let n4 = graph.add_node(TaskId::Render(0, 4), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[n2, n3]);
    let n5 = graph.add_node(TaskId::Render(0, 5), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n6 = graph.add_node(TaskId::Render(0, 6), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n3, n5]);
    let n7 = graph.add_node(TaskId::Render(0, 7), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n2, n4, n6]);
    let n8 = graph.add_node(TaskId::Render(0, 8), TargetKind::Color, size2(800, 600), AllocKind::Fixed(TextureId(100), point2(0, 0)), &[n7]);

    graph.add_root(n5);
    graph.add_root(n8);

    for &with_deallocations in &[true] {
        for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
            build_and_print_graph(
                &graph,
                BuilderOptions {
                    targets: target_option,
                },
                with_deallocations,
            )
        }
    }
}

#[test]
fn test_stacked_shadows() {
    let mut graph = Graph::new();

    const PICTURE: u16 = 0;
    const DOWNSCALE: u16 = 1;
    const BLUR: u16 = 2;

    let pic1 = graph.add_node(TaskId::Render(PICTURE, 1), TargetKind::Color, size2(400, 200), AllocKind::Dynamic, &[]);
    let ds1 = graph.add_node(TaskId::Render(DOWNSCALE, 0), TargetKind::Color, size2(200, 100), AllocKind::Dynamic, &[pic1]);
    let ds2 = graph.add_node(TaskId::Render(DOWNSCALE, 1), TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds1]);
    let vblur1 = graph.add_node(TaskId::Render(BLUR, 0), TargetKind::Color, size2(400, 300), AllocKind::Dynamic, &[ds2]);
    let hblur1 = graph.add_node(TaskId::Render(BLUR, 1), TargetKind::Color, size2(500, 300), AllocKind::Dynamic, &[vblur1]);

    let vblur2 = graph.add_node(TaskId::Render(BLUR, 2), TargetKind::Color, size2(400, 350), AllocKind::Fixed(TextureId(1337), point2(0, 0)), &[ds2]);
    let hblur2 = graph.add_node(TaskId::Render(BLUR, 3), TargetKind::Color, size2(550, 350), AllocKind::Dynamic, &[vblur2]);

    let vblur3 = graph.add_node(TaskId::Render(BLUR, 4), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[ds1]);
    let hblur3 = graph.add_node(TaskId::Render(BLUR, 5), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[vblur3]);

    let ds3 = graph.add_node(TaskId::Render(DOWNSCALE, 2), TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds2]);
    let vblur4 = graph.add_node(TaskId::Render(BLUR, 3), TargetKind::Color, size2(500, 400), AllocKind::Dynamic, &[ds3]);
    let hblur4 = graph.add_node(TaskId::Render(BLUR, 4), TargetKind::Color, size2(600, 400), AllocKind::Dynamic, &[vblur4]);

    let root = graph.add_node(
        TaskId::Render(PICTURE, 0),
        TargetKind::Color,
        size2(1000, 1000),
        AllocKind::Fixed(TextureId(123), point2(0, 0)),
        &[pic1, hblur1, hblur2, hblur3, hblur4]
    );
    graph.add_root(root);

    for &with_deallocations in &[false, true] {
        for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
            build_and_print_graph(
                &graph,
                BuilderOptions {
                    targets: target_option,
                },
                with_deallocations,
            )
        }
    }
}
