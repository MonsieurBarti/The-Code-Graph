package impact

import (
	"sort"
)

// FileNode represents a file and the paths it imports.
type FileNode struct {
	Path    string
	Imports []string
	Symbols []string
}

// ImpactReport describes the blast radius of a changed file.
type ImpactReport struct {
	ChangedFile         string
	DirectDependents    []string
	TransitiveDependents []string
}

// TotalAffected returns the count of unique affected files.
func (r *ImpactReport) TotalAffected() int {
	seen := make(map[string]bool)
	for _, p := range append(r.DirectDependents, r.TransitiveDependents...) { seen[p] = true }
	return len(seen)
}

// Analyzer builds and queries the reverse dependency graph.
type Analyzer struct {
	nodes   map[string]*FileNode
	reverse map[string][]string
}

// New creates a new Analyzer.
func New() *Analyzer {
	return &Analyzer{nodes: make(map[string]*FileNode), reverse: make(map[string][]string)}
}

// Register adds a file node to the graph.
func (a *Analyzer) Register(node *FileNode) {
	a.nodes[node.Path] = node
	for _, imp := range node.Imports {
		a.reverse[imp] = append(a.reverse[imp], node.Path)
	}
}

// ComputeImpact returns the full impact report for a changed file.
func (a *Analyzer) ComputeImpact(path string) *ImpactReport {
	direct := a.reverse[path]
	sort.Strings(direct)
	visited := make(map[string]bool)
	for _, d := range direct { visited[d] = true }
	visited[path] = true
	queue := append([]string{}, direct...)
	var transitive []string
	for len(queue) > 0 {
		cur := queue[0]; queue = queue[1:]
		for _, dep := range a.reverse[cur] {
			if !visited[dep] {
				visited[dep] = true
				transitive = append(transitive, dep)
				queue = append(queue, dep)
			}
		}
	}
	sort.Strings(transitive)
	return &ImpactReport{ChangedFile: path, DirectDependents: direct, TransitiveDependents: transitive}
}

// MostImpactful returns the top N files sorted by direct dependent count.
func (a *Analyzer) MostImpactful(n int) []string {
	type item struct{ path string; count int }
	items := make([]item, 0, len(a.nodes))
	for p := range a.nodes { items = append(items, item{path: p, count: len(a.reverse[p])}) }
	sort.Slice(items, func(i, j int) bool { return items[i].count > items[j].count })
	if n > len(items) { n = len(items) }
	result := make([]string, n)
	for i := 0; i < n; i++ { result[i] = items[i].path }
	return result
}
