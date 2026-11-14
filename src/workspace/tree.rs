//! Binary split tree: how a tab's area is divided between its panes.
//!
//! Every inner node splits its rectangle in two along one axis, at a ratio of the
//! length left once the divider is taken out. Positions are rounded to whole cells
//! when laid out, so the stored ratio survives window resizes without drifting.

use crate::i18n::Direction;
use crate::layout::{CellSize, Rect};
use crate::terminal::PaneId;

/// Fewest grid columns a pane may be split down to.
pub const MIN_PANE_COLUMNS: usize = 10;

/// Fewest grid lines a pane may be split down to.
pub const MIN_PANE_LINES: usize = 3;

/// Distance on each side of a divider that still hits it, in physical pixels.
pub const DIVIDER_HIT_MARGIN: f32 = 3.0;

/// Tolerance on pixel comparisons, which go through float sums.
const EPSILON: f32 = 0.01;

/// Axis a split divides its rectangle along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Children side by side, separated by a vertical divider.
    Horizontal,
    /// Children stacked, separated by a horizontal divider.
    Vertical,
}

impl Axis {
    /// The axis a focus or resize in `direction` moves along.
    pub fn of(direction: Direction) -> Self {
        match direction {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }

    /// Length of one cell along the axis.
    fn cell(self, cell: CellSize) -> f32 {
        match self {
            Axis::Horizontal => cell.width,
            Axis::Vertical => cell.height,
        }
    }

    /// Smallest length of a pane along the axis, in pixels.
    fn min_pane(self, cell: CellSize) -> f32 {
        match self {
            Axis::Horizontal => MIN_PANE_COLUMNS as f32 * cell.width,
            Axis::Vertical => MIN_PANE_LINES as f32 * cell.height,
        }
    }

    /// Start of `rect` along the axis.
    fn start(self, rect: Rect) -> f32 {
        match self {
            Axis::Horizontal => rect.x,
            Axis::Vertical => rect.y,
        }
    }

    /// Length of `rect` along the axis.
    fn length(self, rect: Rect) -> f32 {
        match self {
            Axis::Horizontal => rect.width,
            Axis::Vertical => rect.height,
        }
    }

    /// The other axis.
    fn cross(self) -> Self {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    /// `rect` cut along the axis into a first part of `first` pixels, the divider of
    /// `divider` pixels, and the rest.
    fn cut(self, rect: Rect, first: f32, divider: f32) -> (Rect, Rect, Rect) {
        match self {
            Axis::Horizontal => {
                let line = Rect {
                    x: rect.x + first,
                    width: divider,
                    ..rect
                };
                let second_x = line.x + divider;
                (
                    Rect {
                        width: first,
                        ..rect
                    },
                    line,
                    Rect {
                        x: second_x,
                        width: (rect.x + rect.width - second_x).max(0.0),
                        ..rect
                    },
                )
            }
            Axis::Vertical => {
                let line = Rect {
                    y: rect.y + first,
                    height: divider,
                    ..rect
                };
                let second_y = line.y + divider;
                (
                    Rect {
                        height: first,
                        ..rect
                    },
                    line,
                    Rect {
                        y: second_y,
                        height: (rect.y + rect.height - second_y).max(0.0),
                        ..rect
                    },
                )
            }
        }
    }
}

/// Child of a split, a step of a [`NodePath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    First,
    Second,
}

/// Steps from the root down to a node.
pub type NodePath = Vec<Branch>;

/// Size inputs of a layout: the area divided, the cell size and the divider thickness,
/// all in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub area: Rect,
    pub cell: CellSize,
    pub divider: f32,
}

/// The line between the two children of a split.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    /// Path of the split it belongs to.
    pub node_path: NodePath,
    pub axis: Axis,
    /// Drawn rectangle, the divider's thickness across the axis.
    pub rect: Rect,
}

impl Divider {
    /// Whether the pixel (`x`, `y`) grabs the divider: its rectangle widened by
    /// [`DIVIDER_HIT_MARGIN`] on each side along its axis.
    pub fn hit(&self, x: f32, y: f32) -> bool {
        let r = self.rect;
        let area = match self.axis {
            Axis::Horizontal => Rect {
                x: r.x - DIVIDER_HIT_MARGIN,
                width: r.width + 2.0 * DIVIDER_HIT_MARGIN,
                ..r
            },
            Axis::Vertical => Rect {
                y: r.y - DIVIDER_HIT_MARGIN,
                height: r.height + 2.0 * DIVIDER_HIT_MARGIN,
                ..r
            },
        };
        area.contains(x, y)
    }
}

/// Rectangles of a tree's panes and dividers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// Panes in depth-first order.
    pub panes: Vec<(PaneId, Rect)>,
    pub dividers: Vec<Divider>,
    /// Divider thickness the layout was computed with.
    pub divider: f32,
}

impl Layout {
    /// Rectangle of the pane `id`.
    pub fn rect_of(&self, id: PaneId) -> Option<Rect> {
        self.panes
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
    }

    /// The pane under the pixel (`x`, `y`).
    pub fn pane_at(&self, x: f32, y: f32) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(pane, _)| *pane)
    }

    /// The divider the pixel (`x`, `y`) grabs.
    pub fn divider_at(&self, x: f32, y: f32) -> Option<&Divider> {
        self.dividers.iter().find(|divider| divider.hit(x, y))
    }
}

/// A node of the split tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        /// Share of the length (divider excluded) given to `first`.
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    /// Panes in depth-first order.
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    /// Whether the pane `id` is in the tree.
    pub fn contains(&self, id: PaneId) -> bool {
        self.path_of(id).is_some()
    }

    /// Path to the leaf of the pane `id`.
    pub fn path_of(&self, id: PaneId) -> Option<NodePath> {
        match self {
            Node::Leaf(leaf) => (*leaf == id).then(Vec::new),
            Node::Split { first, second, .. } => {
                let (branch, path) = first
                    .path_of(id)
                    .map(|path| (Branch::First, path))
                    .or_else(|| second.path_of(id).map(|path| (Branch::Second, path)))?;
                let mut full = Vec::with_capacity(path.len() + 1);
                full.push(branch);
                full.extend(path);
                Some(full)
            }
        }
    }

    /// The node at `path`.
    pub fn node(&self, path: &[Branch]) -> Option<&Node> {
        let Some((step, rest)) = path.split_first() else {
            return Some(self);
        };
        match (self, step) {
            (Node::Split { first, .. }, Branch::First) => first.node(rest),
            (Node::Split { second, .. }, Branch::Second) => second.node(rest),
            (Node::Leaf(_), _) => None,
        }
    }

    fn node_mut(&mut self, path: &[Branch]) -> Option<&mut Node> {
        let Some((step, rest)) = path.split_first() else {
            return Some(self);
        };
        match (self, step) {
            (Node::Split { first, .. }, Branch::First) => first.node_mut(rest),
            (Node::Split { second, .. }, Branch::Second) => second.node_mut(rest),
            (Node::Leaf(_), _) => None,
        }
    }

    /// Splits the pane `target` along `axis` in two halves, the pane `new` second.
    /// Refused (`false`) when `target` is not in the tree or either half would be
    /// under [`MIN_PANE_COLUMNS`] or [`MIN_PANE_LINES`].
    pub fn split(&mut self, target: PaneId, axis: Axis, new: PaneId, geometry: Geometry) -> bool {
        debug_assert!(!self.contains(new), "pane {new:?} is already in the tree");
        let Some(path) = self.path_of(target) else {
            return false;
        };
        let rect = self.rect_at(&path, geometry);
        let cell = axis.cell(geometry.cell);
        let min = axis.min_pane(geometry.cell);
        let available = axis.length(rect) - geometry.divider;
        let first = round_to_cell(available * 0.5, cell);
        if first + EPSILON < min || available - first + EPSILON < min {
            return false;
        }
        if let Some(node) = self.node_mut(&path) {
            *node = Node::Split {
                axis,
                ratio: 0.5,
                first: Box::new(Node::Leaf(target)),
                second: Box::new(Node::Leaf(new)),
            };
        }
        true
    }

    /// Removes the pane `target`: its sibling takes its parent's place. Refused
    /// (`false`) when `target` is not in the tree or is its only pane.
    pub fn remove(&mut self, target: PaneId) -> bool {
        let Some(path) = self.path_of(target) else {
            return false;
        };
        let Some((last, parent_path)) = path.split_last() else {
            return false;
        };
        let Some(parent) = self.node_mut(parent_path) else {
            return false;
        };
        let sibling = match std::mem::replace(parent, Node::Leaf(target)) {
            Node::Split { first, second, .. } => match last {
                Branch::First => *second,
                Branch::Second => *first,
            },
            leaf @ Node::Leaf(_) => leaf,
        };
        *parent = sibling;
        true
    }

    /// Rectangles of the panes and dividers in `geometry.area`.
    pub fn layout(&self, geometry: Geometry) -> Layout {
        let mut layout = Layout {
            divider: geometry.divider,
            ..Layout::default()
        };
        let mut path = Vec::new();
        self.lay_out(geometry.area, geometry, &mut path, &mut layout);
        layout
    }

    fn lay_out(&self, rect: Rect, geometry: Geometry, path: &mut NodePath, out: &mut Layout) {
        match self {
            Node::Leaf(id) => out.panes.push((*id, rect)),
            Node::Split {
                axis,
                first,
                second,
                ..
            } => {
                let (first_rect, line, second_rect) = self.cut(rect, geometry);
                path.push(Branch::First);
                first.lay_out(first_rect, geometry, path, out);
                path.pop();
                out.dividers.push(Divider {
                    node_path: path.clone(),
                    axis: *axis,
                    rect: line,
                });
                path.push(Branch::Second);
                second.lay_out(second_rect, geometry, path, out);
                path.pop();
            }
        }
    }

    /// Rectangle of the node at `path` in `geometry.area`.
    fn rect_at(&self, path: &[Branch], geometry: Geometry) -> Rect {
        let mut node = self;
        let mut rect = geometry.area;
        for step in path {
            let Node::Split { first, second, .. } = node else {
                break;
            };
            let (first_rect, _, second_rect) = node.cut(rect, geometry);
            (node, rect) = match step {
                Branch::First => (first, first_rect),
                Branch::Second => (second, second_rect),
            };
        }
        rect
    }

    /// For a split, `rect` cut into its first child, divider and second child.
    fn cut(&self, rect: Rect, geometry: Geometry) -> (Rect, Rect, Rect) {
        match self {
            Node::Leaf(_) => (rect, Rect::default(), Rect::default()),
            Node::Split { axis, .. } => {
                let first = self.first_length(rect, geometry);
                axis.cut(rect, first, geometry.divider)
            }
        }
    }

    /// For a split laid out in `rect`, length of the first child at its ratio: rounded
    /// to whole cells, then clamped so both children keep their minimum size.
    fn first_length(&self, rect: Rect, geometry: Geometry) -> f32 {
        let Node::Split { axis, ratio, .. } = self else {
            return 0.0;
        };
        let available = (axis.length(rect) - geometry.divider).max(0.0);
        let rounded = round_to_cell(available * ratio, axis.cell(geometry.cell));
        self.clamp_first(rounded, available, geometry)
    }

    /// For a split, `length` clamped so both children keep their minimum size in
    /// `available` pixels. When even that does not fit, only kept within `available`.
    fn clamp_first(&self, length: f32, available: f32, geometry: Geometry) -> f32 {
        let Node::Split {
            axis,
            first,
            second,
            ..
        } = self
        else {
            return length;
        };
        let low = first.min_length(*axis, geometry);
        let high = available - second.min_length(*axis, geometry);
        if low <= high {
            length.clamp(low, high)
        } else {
            length.clamp(0.0, available)
        }
    }

    /// Smallest length of the node along `axis` that keeps every pane at its minimum.
    fn min_length(&self, axis: Axis, geometry: Geometry) -> f32 {
        match self {
            Node::Leaf(_) => axis.min_pane(geometry.cell),
            Node::Split {
                axis: own,
                first,
                second,
                ..
            } => {
                let a = first.min_length(axis, geometry);
                let b = second.min_length(axis, geometry);
                if *own == axis {
                    a + geometry.divider + b
                } else {
                    a.max(b)
                }
            }
        }
    }

    /// Pane after `from` in depth-first order, wrapping to the first.
    pub fn next(&self, from: PaneId) -> Option<PaneId> {
        let leaves = self.leaves();
        let index = leaves.iter().position(|&id| id == from)?;
        Some(leaves[(index + 1) % leaves.len()])
    }

    /// Pane before `from` in depth-first order, wrapping to the last.
    pub fn previous(&self, from: PaneId) -> Option<PaneId> {
        let leaves = self.leaves();
        let index = leaves.iter().position(|&id| id == from)?;
        Some(leaves[(index + leaves.len() - 1) % leaves.len()])
    }

    /// Grows the pane `target` by `step` cells toward `direction`: moves the divider
    /// of its nearest ancestor split on that axis where `target` is on the side facing
    /// `direction`, clamped to the minimum sizes. `false` when nothing moved.
    pub fn resize(
        &mut self,
        target: PaneId,
        direction: Direction,
        step: usize,
        geometry: Geometry,
    ) -> bool {
        let Some(path) = self.path_of(target) else {
            return false;
        };
        let axis = Axis::of(direction);
        let (facing, sign) = match direction {
            Direction::Right | Direction::Down => (Branch::First, 1.0),
            Direction::Left | Direction::Up => (Branch::Second, -1.0),
        };
        let ancestor = (0..path.len()).rev().find(|&depth| {
            path[depth] == facing
                && matches!(self.node(&path[..depth]), Some(Node::Split { axis: own, .. }) if *own == axis)
        });
        let Some(depth) = ancestor else {
            return false;
        };
        let split_path = &path[..depth];
        let rect = self.rect_at(split_path, geometry);
        let current = self
            .node(split_path)
            .map_or(0.0, |node| node.first_length(rect, geometry));
        let wanted = current + sign * step as f32 * axis.cell(geometry.cell);
        self.set_first_length(split_path, rect, wanted, geometry)
    }

    /// Moves the divider of the split at `node_path` to the pixel `position` along its
    /// axis, rounded to a whole cell and clamped to the minimum sizes. `false` when
    /// nothing moved.
    pub fn set_divider(&mut self, node_path: &[Branch], position: f32, geometry: Geometry) -> bool {
        let Some(Node::Split { axis, .. }) = self.node(node_path) else {
            return false;
        };
        let axis = *axis;
        let rect = self.rect_at(node_path, geometry);
        let wanted = round_to_cell(position - axis.start(rect), axis.cell(geometry.cell));
        self.set_first_length(node_path, rect, wanted, geometry)
    }

    /// Gives the split at `path`, laid out in `rect`, a first child of `length` pixels
    /// (clamped). `false` when the laid out position does not change.
    fn set_first_length(
        &mut self,
        path: &[Branch],
        rect: Rect,
        length: f32,
        geometry: Geometry,
    ) -> bool {
        let Some(node) = self.node_mut(path) else {
            return false;
        };
        let Node::Split { axis, .. } = &*node else {
            return false;
        };
        let available = axis.length(rect) - geometry.divider;
        if available <= 0.0 {
            return false;
        }
        let before = node.first_length(rect, geometry);
        let clamped = node.clamp_first(length, available, geometry);
        if let Node::Split { ratio, .. } = node {
            *ratio = clamped / available;
        }
        (node.first_length(rect, geometry) - before).abs() > EPSILON
    }

    /// Sets every ratio so panes along the same axis get equal lengths: each child
    /// weighs the number of panes it lines up along its parent's axis.
    pub fn equalise(&mut self) {
        if let Node::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            first.equalise();
            second.equalise();
            let a = first.count_along(*axis) as f32;
            let b = second.count_along(*axis) as f32;
            *ratio = a / (a + b);
        }
    }

    /// Number of panes lined up along `axis`.
    fn count_along(&self, axis: Axis) -> usize {
        match self {
            Node::Leaf(_) => 1,
            Node::Split {
                axis: own,
                first,
                second,
                ..
            } => {
                let a = first.count_along(axis);
                let b = second.count_along(axis);
                if *own == axis { a + b } else { a.max(b) }
            }
        }
    }
}

/// `length` rounded to a whole number of cells of `cell` pixels.
fn round_to_cell(length: f32, cell: f32) -> f32 {
    if cell <= 0.0 {
        return length;
    }
    (length / cell).round() * cell
}

/// The pane next to `from` in `direction`: among the panes whose edge touches
/// `from`'s edge on that side, the one overlapping it the most along the other axis,
/// ties broken by the one containing `from`'s centre line.
pub fn neighbour(from: PaneId, direction: Direction, layout: &Layout) -> Option<PaneId> {
    let origin = layout.rect_of(from)?;
    let axis = Axis::of(direction);
    let cross = axis.cross();
    let forward = matches!(direction, Direction::Right | Direction::Down);
    let centre = cross.start(origin) + cross.length(origin) / 2.0;
    let mut best: Option<(PaneId, f32, bool)> = None;
    for &(id, rect) in &layout.panes {
        if id == from {
            continue;
        }
        let gap = if forward {
            axis.start(rect) - (axis.start(origin) + axis.length(origin))
        } else {
            axis.start(origin) - (axis.start(rect) + axis.length(rect))
        };
        if gap < -EPSILON || gap > layout.divider + EPSILON {
            continue;
        }
        let low = cross.start(rect).max(cross.start(origin));
        let high = (cross.start(rect) + cross.length(rect))
            .min(cross.start(origin) + cross.length(origin));
        let overlap = high - low;
        if overlap <= EPSILON {
            continue;
        }
        let holds_centre =
            centre >= cross.start(rect) && centre < cross.start(rect) + cross.length(rect);
        let better = match best {
            None => true,
            Some((_, best_overlap, best_centre)) => {
                overlap > best_overlap + EPSILON
                    || ((overlap - best_overlap).abs() <= EPSILON && holds_centre && !best_centre)
            }
        };
        if better {
            best = Some((id, overlap, holds_centre));
        }
    }
    best.map(|(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: CellSize = CellSize {
        width: 10.0,
        height: 20.0,
    };

    fn id(n: u64) -> PaneId {
        PaneId(n)
    }

    fn geometry(width: f32, height: f32) -> Geometry {
        Geometry {
            area: Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            cell: CELL,
            divider: 1.0,
        }
    }

    fn rect(layout: &Layout, n: u64) -> Rect {
        layout.rect_of(id(n)).expect("pane in layout")
    }

    /// Pane 1 on the left, the full height; panes 2 (above) and 3 (below) on the right.
    fn l_shape(g: Geometry) -> Node {
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Horizontal, id(2), g));
        assert!(tree.split(id(2), Axis::Vertical, id(3), g));
        tree
    }

    #[test]
    fn split_replaces_the_leaf_and_keeps_the_others() {
        let g = geometry(1001.0, 601.0);
        let tree = l_shape(g);
        assert_eq!(tree.leaves(), vec![id(1), id(2), id(3)]);
        assert_eq!(
            tree,
            Node::Split {
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(Node::Leaf(id(1))),
                second: Box::new(Node::Split {
                    axis: Axis::Vertical,
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(id(2))),
                    second: Box::new(Node::Leaf(id(3))),
                }),
            }
        );
    }

    #[test]
    fn split_of_a_missing_pane_is_refused() {
        let mut tree = Node::Leaf(id(1));
        assert!(!tree.split(id(9), Axis::Horizontal, id(2), geometry(1001.0, 601.0)));
        assert_eq!(tree, Node::Leaf(id(1)));
    }

    #[test]
    fn remove_puts_the_sibling_in_place_of_the_parent() {
        let mut tree = l_shape(geometry(1001.0, 601.0));
        assert!(tree.remove(id(2)));
        assert_eq!(tree.leaves(), vec![id(1), id(3)]);
        assert!(matches!(
            &tree,
            Node::Split { axis: Axis::Horizontal, first, second, .. }
                if **first == Node::Leaf(id(1)) && **second == Node::Leaf(id(3))
        ));
        assert!(tree.remove(id(1)));
        assert_eq!(tree, Node::Leaf(id(3)));
    }

    #[test]
    fn remove_of_the_only_or_a_missing_pane_is_refused() {
        let mut tree = Node::Leaf(id(1));
        assert!(!tree.remove(id(1)));
        assert!(!tree.remove(id(2)));
        assert_eq!(tree, Node::Leaf(id(1)));
    }

    #[test]
    fn split_is_refused_under_the_minimum_size() {
        // 201 px: 200 px left once the divider is out, two halves of 10 columns.
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Horizontal, id(2), geometry(201.0, 601.0)));
        // 200 px: one half would be 99 px, under 10 columns.
        let mut tree = Node::Leaf(id(1));
        assert!(!tree.split(id(1), Axis::Horizontal, id(2), geometry(200.0, 601.0)));
        assert_eq!(tree, Node::Leaf(id(1)));
        // 121 px high: two halves of 3 lines; 120 px is too short.
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Vertical, id(2), geometry(1001.0, 121.0)));
        let mut tree = Node::Leaf(id(1));
        assert!(!tree.split(id(1), Axis::Vertical, id(2), geometry(1001.0, 120.0)));
    }

    #[test]
    fn single_pane_fills_the_area_without_divider() {
        let g = geometry(1001.0, 601.0);
        let layout = Node::Leaf(id(1)).layout(g);
        assert_eq!(layout.panes, vec![(id(1), g.area)]);
        assert!(layout.dividers.is_empty());
    }

    #[test]
    fn layout_fills_the_area_exactly() {
        let mut g = geometry(1001.0, 601.0);
        g.area.x = 5.0;
        g.area.y = 7.0;
        let layout = l_shape(g).layout(g);
        let (left, top, bottom) = (rect(&layout, 1), rect(&layout, 2), rect(&layout, 3));

        assert_eq!((left.x, left.y, left.height), (5.0, 7.0, 601.0));
        assert_eq!(left.width + 1.0 + top.width, 1001.0);
        assert_eq!(top.x, left.x + left.width + 1.0);
        assert_eq!(top.x + top.width, 5.0 + 1001.0);
        assert_eq!(bottom.x, top.x);
        assert_eq!(bottom.width, top.width);
        assert_eq!(top.y, 7.0);
        assert_eq!(top.height + 1.0 + bottom.height, 601.0);
        assert_eq!(bottom.y + bottom.height, 7.0 + 601.0);

        assert_eq!(layout.dividers.len(), 2);
        let root = &layout.dividers[0];
        assert_eq!(root.node_path, Vec::<Branch>::new());
        assert_eq!(root.axis, Axis::Horizontal);
        assert_eq!(
            root.rect,
            Rect {
                x: left.x + left.width,
                y: 7.0,
                width: 1.0,
                height: 601.0
            }
        );
        let inner = &layout.dividers[1];
        assert_eq!(inner.node_path, vec![Branch::Second]);
        assert_eq!(inner.axis, Axis::Vertical);
        assert_eq!(
            inner.rect,
            Rect {
                x: top.x,
                y: top.y + top.height,
                width: top.width,
                height: 1.0
            }
        );
    }

    #[test]
    fn layout_rounds_split_positions_to_whole_cells() {
        let g = geometry(1001.0, 601.0);
        let tree = Node::Split {
            axis: Axis::Horizontal,
            ratio: 0.333,
            first: Box::new(Node::Leaf(id(1))),
            second: Box::new(Node::Leaf(id(2))),
        };
        let layout = tree.layout(g);
        // 1000 px * 0.333 = 333 px, rounded to 33 cells.
        assert_eq!(rect(&layout, 1).width, 330.0);
        assert_eq!(rect(&layout, 2).width, 670.0);
    }

    #[test]
    fn divider_hit_area_spreads_three_pixels_each_side() {
        let g = geometry(1001.0, 601.0);
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Horizontal, id(2), g));
        let layout = tree.layout(g);
        let divider = &layout.dividers[0];
        assert_eq!(divider.rect.x, 500.0);
        assert!(divider.hit(497.0, 10.0));
        assert!(divider.hit(503.9, 10.0));
        assert!(!divider.hit(496.9, 10.0));
        assert!(!divider.hit(504.0, 10.0));
        assert!(layout.divider_at(502.0, 10.0).is_some());
        assert_eq!(layout.pane_at(502.0, 10.0), Some(id(2)));
    }

    #[test]
    fn neighbour_in_the_four_directions_on_an_l_shape() {
        let g = geometry(1001.0, 601.0);
        let mut tree = l_shape(g);
        // Pane 3 taller than pane 2, so it overlaps pane 1 the most.
        assert!(tree.set_divider(&[Branch::Second], 180.0, g));
        let layout = tree.layout(g);
        assert_eq!(rect(&layout, 2).height, 180.0);

        assert_eq!(neighbour(id(1), Direction::Right, &layout), Some(id(3)));
        assert_eq!(neighbour(id(2), Direction::Left, &layout), Some(id(1)));
        assert_eq!(neighbour(id(3), Direction::Left, &layout), Some(id(1)));
        assert_eq!(neighbour(id(2), Direction::Down, &layout), Some(id(3)));
        assert_eq!(neighbour(id(3), Direction::Up, &layout), Some(id(2)));
        assert_eq!(neighbour(id(1), Direction::Left, &layout), None);
        assert_eq!(neighbour(id(1), Direction::Up, &layout), None);
        assert_eq!(neighbour(id(2), Direction::Right, &layout), None);
        assert_eq!(neighbour(id(3), Direction::Down, &layout), None);
    }

    #[test]
    fn neighbour_tie_goes_to_the_pane_on_the_centre_line() {
        let square = |x, y, height| Rect {
            x,
            y,
            width: 100.0,
            height,
        };
        let layout = Layout {
            panes: vec![
                (id(1), square(0.0, 0.0, 100.0)),
                (id(2), square(101.0, 0.0, 50.0)),
                (id(3), square(101.0, 50.0, 50.0)),
            ],
            dividers: Vec::new(),
            divider: 1.0,
        };
        // Both overlap pane 1 by 50 px; its centre line (y = 50) is in pane 3.
        assert_eq!(neighbour(id(1), Direction::Right, &layout), Some(id(3)));
    }

    #[test]
    fn resize_moves_the_nearest_ancestor_on_its_axis() {
        let g = geometry(1001.0, 601.0);
        let mut tree = l_shape(g);
        // Pane 3 is below pane 2: the nearest split facing left is the root.
        assert!(tree.resize(id(3), Direction::Left, 2, g));
        assert_eq!(rect(&tree.layout(g), 1).width, 480.0);
        // Pane 2 grows down: the inner vertical split.
        assert!(tree.resize(id(2), Direction::Down, 1, g));
        let layout = tree.layout(g);
        assert_eq!(rect(&layout, 2).height, 320.0);
        assert_eq!(rect(&layout, 1).width, 480.0);
        // No split on pane 1's left.
        assert!(!tree.resize(id(1), Direction::Left, 1, g));
        assert!(!tree.resize(id(1), Direction::Up, 1, g));
    }

    #[test]
    fn resize_clamps_to_the_minimum_sizes() {
        let g = geometry(1001.0, 601.0);
        let mut tree = l_shape(g);
        assert!(tree.resize(id(1), Direction::Right, 1000, g));
        let layout = tree.layout(g);
        assert_eq!(rect(&layout, 2).width, 100.0);
        assert_eq!(rect(&layout, 1).width, 900.0);
        assert!(!tree.resize(id(1), Direction::Right, 1, g));
        assert!(tree.resize(id(3), Direction::Left, 1000, g));
        assert_eq!(rect(&tree.layout(g), 1).width, 100.0);
        assert!(tree.resize(id(3), Direction::Up, 1000, g));
        assert_eq!(rect(&tree.layout(g), 2).height, 60.0);
    }

    #[test]
    fn set_divider_rounds_and_clamps() {
        let g = geometry(1001.0, 601.0);
        let mut tree = l_shape(g);
        assert!(tree.set_divider(&[], 304.0, g));
        assert_eq!(rect(&tree.layout(g), 1).width, 300.0);
        assert!(tree.set_divider(&[], 5.0, g));
        assert_eq!(rect(&tree.layout(g), 1).width, 100.0);
        assert!(!tree.set_divider(&[Branch::First], 5.0, g));
    }

    #[test]
    fn equalise_gives_three_columns_the_same_width() {
        // 992 px: 991 px left at the root, 330 + 1 + 661, then 330 + 1 + 330.
        let g = geometry(992.0, 601.0);
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Horizontal, id(2), g));
        assert!(tree.split(id(2), Axis::Horizontal, id(3), g));
        let before = tree.layout(g);
        assert_ne!(rect(&before, 1).width, rect(&before, 2).width);
        tree.equalise();
        let layout = tree.layout(g);
        assert_eq!(rect(&layout, 1).width, 330.0);
        assert_eq!(rect(&layout, 2).width, 330.0);
        assert_eq!(rect(&layout, 3).width, 330.0);
    }

    #[test]
    fn equalise_counts_columns_of_a_stacked_child() {
        // Pane 1 | (panes 2 | 3 above pane 4): the right child spans two columns.
        let g = geometry(1001.0, 601.0);
        let mut tree = Node::Leaf(id(1));
        assert!(tree.split(id(1), Axis::Horizontal, id(2), g));
        assert!(tree.split(id(2), Axis::Vertical, id(4), g));
        assert!(tree.split(id(2), Axis::Horizontal, id(3), g));
        tree.equalise();
        let Node::Split { ratio, .. } = tree else {
            panic!("root is a split");
        };
        assert!((ratio - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn next_and_previous_wrap() {
        let tree = l_shape(geometry(1001.0, 601.0));
        assert_eq!(tree.next(id(1)), Some(id(2)));
        assert_eq!(tree.next(id(3)), Some(id(1)));
        assert_eq!(tree.previous(id(1)), Some(id(3)));
        assert_eq!(tree.previous(id(2)), Some(id(1)));
        assert_eq!(tree.next(id(9)), None);
        let single = Node::Leaf(id(1));
        assert_eq!(single.next(id(1)), Some(id(1)));
        assert_eq!(single.previous(id(1)), Some(id(1)));
    }
}
