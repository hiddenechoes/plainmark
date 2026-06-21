import type { TreeNode } from "../lib/tauri";

interface FileTreeProps {
  nodes: TreeNode[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
}

export function FileTree({ nodes, selectedPath, onSelect }: FileTreeProps) {
  if (nodes.length === 0) {
    return <p className="tree-empty">No markdown files in this vault yet.</p>;
  }
  return (
    <ul className="tree">
      {nodes.map((node) => (
        <TreeItem key={node.path} node={node} selectedPath={selectedPath} onSelect={onSelect} />
      ))}
    </ul>
  );
}

function TreeItem({
  node,
  selectedPath,
  onSelect,
}: {
  node: TreeNode;
  selectedPath: string | null;
  onSelect: (path: string) => void;
}) {
  if (node.isDir) {
    return (
      <li>
        <span className="tree-folder">{node.name}</span>
        <ul className="tree">
          {node.children.map((child) => (
            <TreeItem
              key={child.path}
              node={child}
              selectedPath={selectedPath}
              onSelect={onSelect}
            />
          ))}
        </ul>
      </li>
    );
  }
  return (
    <li>
      <button
        type="button"
        className={node.path === selectedPath ? "tree-file selected" : "tree-file"}
        onClick={() => onSelect(node.path)}
      >
        {node.name}
      </button>
    </li>
  );
}
