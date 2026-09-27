import { useState, useCallback, useEffect } from 'react';
import { 
  ReactFlow, 
  Controls, 
  Background, 
  applyNodeChanges, 
  applyEdgeChanges,
  BackgroundVariant,
  Panel
} from '@xyflow/react';
import type { Node, Edge, OnNodesChange, OnEdgesChange, NodeChange, EdgeChange } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import dagre from 'dagre';
import { Loader2, Zap } from 'lucide-react';

const dagreGraph = new dagre.graphlib.Graph();
dagreGraph.setDefaultEdgeLabel(() => ({}));

// Helper to calculate layout
const getLayoutedElements = (nodes: Node[], edges: Edge[], direction = 'TB') => {
  dagreGraph.setGraph({ rankdir: direction, nodesep: 60, ranksep: 100 });

  nodes.forEach((node) => {
    // approx sizes
    dagreGraph.setNode(node.id, { width: 180, height: 60 });
  });

  edges.forEach((edge) => {
    dagreGraph.setEdge(edge.source, edge.target);
  });

  dagre.layout(dagreGraph);

  const newNodes = nodes.map((node) => {
    const nodeWithPosition = dagreGraph.node(node.id);
    return {
      ...node,
      position: {
        x: nodeWithPosition.x - 90,
        y: nodeWithPosition.y - 30,
      },
    };
  });

  return { nodes: newNodes, edges };
};

export default function GraphViewer() {
  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const onNodesChange: OnNodesChange = useCallback(
    (changes: NodeChange[]) => setNodes((nds) => applyNodeChanges(changes, nds)),
    []
  );
  
  const onEdgesChange: OnEdgesChange = useCallback(
    (changes: EdgeChange[]) => setEdges((eds) => applyEdgeChanges(changes, eds)),
    []
  );

  useEffect(() => {
    async function fetchGraph() {
      try {
        const res = await fetch('/api/graph');
        const data = await res.json();

        // Map backend API nodes to ReactFlow format
        const rfNodes = data.nodes.map((n: any) => {
          const isTainted = n.data.isTainted;
          const bg = isTainted ? '#2a0a0a' : '#1a1b26';
          const border = isTainted ? '1px solid #ff073a' : '1px solid #333';
          const color = isTainted ? '#ff8a8a' : '#c0caf5';
          const shadow = isTainted ? '0 0 15px rgba(255, 7, 58, 0.5)' : 'none';

          let icon = '📦';
          if (n.data.kind === 'Function') icon = 'ƒ()';
          if (n.data.kind === 'Endpoint') icon = '🌐';
          if (n.data.kind === 'Struct') icon = '🏗️';

          return {
            id: n.id,
            position: { x: 0, y: 0 },
            data: { 
              label: (
                <div className="flex flex-col items-center">
                  <span className="text-xs text-gray-400 mb-1">{icon} {n.data.kind}</span>
                  <span className="font-mono text-xs font-semibold">{n.data.label}</span>
                  {isTainted && <span className="text-[10px] text-[#ff073a] mt-1 font-bold animate-pulse">BLAST RADIUS</span>}
                </div>
              )
            },
            style: { 
              background: bg, 
              color, 
              border, 
              boxShadow: shadow,
              borderRadius: '8px',
              padding: '10px',
              width: 180
            }
          };
        });

        const rfEdges = data.edges.map((e: any) => ({
          ...e,
          style: { stroke: '#555', strokeWidth: 2 },
          animated: true,
        }));

        const { nodes: layoutedNodes, edges: layoutedEdges } = getLayoutedElements(rfNodes, rfEdges);
        
        setNodes(layoutedNodes);
        setEdges(layoutedEdges);
        setIsLoading(false);
      } catch (e) {
        console.error("Failed to load graph", e);
        setIsLoading(false);
      }
    }

    fetchGraph();
  }, []);

  if (isLoading) {
    return (
      <div className="w-full h-full flex flex-col items-center justify-center text-gray-400">
        <Loader2 className="animate-spin mb-4" size={32} />
        <p>Analyzing Architecture Blast Radius...</p>
      </div>
    );
  }

  return (
    <div className="w-full h-full min-h-[600px] border border-[rgba(255,255,255,0.1)] rounded-xl overflow-hidden glass-panel relative">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        fitView
        className="bg-[#0a0a0f]"
      >
        <Background variant={BackgroundVariant.Dots} gap={16} size={1} color="rgba(255,255,255,0.05)" />
        <Controls className="bg-black/50 border-none fill-white" />
        <Panel position="top-left" className="bg-black/80 px-4 py-2 rounded-lg border border-[rgba(255,10,50,0.3)] shadow-[0_0_15px_rgba(255,10,50,0.2)]">
          <div className="flex items-center space-x-2 text-white">
            <Zap className="text-[#ff073a] animate-pulse" size={18} />
            <h3 className="font-semibold text-sm tracking-wide">BLAST-RADIUS TRUST DECAY</h3>
          </div>
          <p className="text-xs text-gray-400 mt-1 max-w-[250px]">
            Visualizing impact of vulnerable component: <code className="text-[#ff073a]">unsafe_raw_query()</code> across the Dependency Graph.
          </p>
        </Panel>
      </ReactFlow>
    </div>
  );
}
