export interface ProcessSearchNode {
  pid: number
  name: string
  hijacked: boolean
  cmdline?: string | null
  image_path?: string | null
  children?: ProcessSearchNode[]
}

export function filterProcessTree<T extends ProcessSearchNode>(
  nodes: T[], query: string, tab: 'all' | 'active' | 'hijacked', activePids: Set<number>,
): T[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (!terms.length && tab === 'all') return nodes
  const visit = (items: T[]): T[] => items.flatMap((node) => {
    const children = visit((node.children ?? []) as T[])
    const text = `${node.name} ${node.pid} ${node.image_path ?? ''} ${node.cmdline ?? ''}`.toLowerCase()
    const matches = terms.every((term) => text.includes(term)) &&
      (tab === 'all' || (tab === 'active' ? activePids.has(node.pid) : node.hijacked))
    return matches || children.length ? [{ ...node, children }] : []
  })
  return visit(nodes)
}
