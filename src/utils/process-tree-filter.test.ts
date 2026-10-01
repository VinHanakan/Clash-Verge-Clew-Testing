import { describe, expect, it } from 'vitest'
import { filterProcessTree, type ProcessSearchNode } from './process-tree-filter'

const tree: ProcessSearchNode[] = [{ pid: 1, name: 'services.exe', hijacked: false, children: [
  { pid: 2, name: 'codex.exe', hijacked: true, image_path: 'C:\\Apps\\Codex.exe' },
  { pid: 3, name: 'unrelated.exe', hijacked: false },
] }]

describe('process tree search', () => {
  it('keeps ancestor context but removes unrelated siblings', () => {
    const result = filterProcessTree(tree, 'CODEX', 'all', new Set())
    expect(result[0].children?.map((n) => n.pid)).toEqual([2])
    expect(tree[0].children).toHaveLength(2)
  })
  it('combines text with the active or intercepted filter at each node', () => {
    expect(filterProcessTree(tree, 'codex', 'active', new Set([3]))).toEqual([])
    expect(filterProcessTree(tree, 'C:\\Apps', 'hijacked', new Set())[0].children?.[0].pid).toBe(2)
  })
})
