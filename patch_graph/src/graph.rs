use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableGraph;

use crate::node::{EdgeData, Node};

pub type NodeId = NodeIndex;
pub type EdgeId = EdgeIndex;
pub type PatchGraph = StableGraph<Node, EdgeData>;
