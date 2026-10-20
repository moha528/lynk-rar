import type { Entry } from "./api";

export interface TreeNode {
  name: string;
  /** Chemin complet dans l'archive, séparé par `/`. "" pour la racine. */
  path: string;
  isDir: boolean;
  size: number;
  packed: number | null;
  modified: number | null;
  encrypted: boolean;
  children: Map<string, TreeNode> | null;
  parent: TreeNode | null;
}

function node(name: string, path: string, isDir: boolean, parent: TreeNode | null): TreeNode {
  return {
    name,
    path,
    isDir,
    size: 0,
    packed: null,
    modified: null,
    encrypted: false,
    children: isDir ? new Map() : null,
    parent,
  };
}

/**
 * Arborescence à partir de la liste plate. Les dossiers implicites (présents
 * seulement dans le chemin de leurs fichiers, fréquent en ZIP et TAR) sont créés.
 */
export function buildTree(entries: Entry[]): TreeNode {
  const root = node("", "", true, null);
  for (const e of entries) {
    const parts = e.path.split("/").filter((p) => p && p !== ".");
    let cur = root;
    for (let i = 0; i < parts.length && cur.children; i++) {
      const last = i === parts.length - 1;
      let child = cur.children.get(parts[i]);
      if (!child) {
        const path = parts.slice(0, i + 1).join("/");
        child = node(parts[i], path, !last || e.isDir, cur);
        cur.children.set(parts[i], child);
      }
      if (last) {
        child.modified = e.modified ?? child.modified;
        child.encrypted = e.encrypted;
        if (!child.isDir) {
          child.size = e.size;
          child.packed = e.packedSize;
        }
      }
      cur = child;
    }
  }
  sum(root);
  return root;
}

function sum(n: TreeNode): void {
  if (!n.children) return;
  let size = 0;
  let packed: number | null = 0;
  for (const c of n.children.values()) {
    sum(c);
    size += c.size;
    packed = packed !== null && c.packed !== null ? packed + c.packed : null;
    n.encrypted ||= c.encrypted;
  }
  n.size = size;
  n.packed = packed;
}

export function find(root: TreeNode, path: string): TreeNode | null {
  let cur: TreeNode | undefined = root;
  for (const part of path.split("/").filter(Boolean)) {
    cur = cur?.children?.get(part);
    if (!cur) return null;
  }
  return cur;
}

export function* walk(n: TreeNode): Generator<TreeNode> {
  for (const c of n.children?.values() ?? []) {
    yield c;
    yield* walk(c);
  }
}

export function countFiles(n: TreeNode): number {
  let count = 0;
  for (const c of walk(n)) if (!c.isDir) count++;
  return count;
}
