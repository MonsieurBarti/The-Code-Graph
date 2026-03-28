package depgraph

import (
	"fmt"
	"sort"
)

// Module represents a code module with its dependencies.
type Module struct {
	Name    string
	Path    string
	Imports []string
}

// DependencyGraph holds the import relationship graph.
type DependencyGraph struct {
	nodes   map[string]*Module
	edges   map[string][]string
	reverse map[string][]string
}

// NewDependencyGraph initializes an empty graph.
func NewDependencyGraph() *DependencyGraph {
	return &DependencyGraph{
		nodes:   make(map[string]*Module),
		edges:   make(map[string][]string),
		reverse: make(map[string][]string),
	}
}

// Add registers a module and its imports.
func (g *DependencyGraph) Add(m *Module) {
	g.nodes[m.Name] = m
	for _, imp := range m.Imports {
		g.edges[m.Name] = append(g.edges[m.Name], imp)
		g.reverse[imp] = append(g.reverse[imp], m.Name)
	}
}

// Dependents returns direct dependents of the given module.
func (g *DependencyGraph) Dependents(name string) []string {
	return g.reverse[name]
}

// ImpactSet computes the transitive set of modules affected by a change.
func (g *DependencyGraph) ImpactSet(name string) []string {
	visited := make(map[string]bool)
	queue := []string{name}
	for len(queue) > 0 {
		cur := queue[0]; queue = queue[1:]
		for _, dep := range g.reverse[cur] {
			if !visited[dep] {
				visited[dep] = true
				queue = append(queue, dep)
			}
		}
	}
	result := make([]string, 0, len(visited))
	for k := range visited { result = append(result, k) }
	sort.Strings(result)
	return result
}

// TopologicalOrder returns a topological sort, or an error if a cycle exists.
func (g *DependencyGraph) TopologicalOrder() ([]string, error) {
	inDegree := make(map[string]int)
	for name := range g.nodes { inDegree[name] = 0 }
	for _, targets := range g.edges {
		for _, t := range targets { inDegree[t]++ }
	}
	var queue []string
	for name, deg := range inDegree { if deg == 0 { queue = append(queue, name) } }
	sort.Strings(queue)
	var order []string
	for len(queue) > 0 {
		cur := queue[0]; queue = queue[1:]
		order = append(order, cur)
		for _, dep := range g.edges[cur] {
			inDegree[dep]--
			if inDegree[dep] == 0 { queue = append(queue, dep); sort.Strings(queue) }
		}
	}
	if len(order) != len(g.nodes) { return nil, fmt.Errorf("cycle detected") }
	return order, nil
}
