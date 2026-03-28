package indexer

import (
	"crypto/md5"
	"encoding/hex"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

// IndexEntry holds parsed information for a single file.
type IndexEntry struct {
	Path     string
	Symbols  []string
	Imports  []string
	Checksum string
	Lines    int
}

// IndexStats summarizes an indexing run.
type IndexStats struct {
	FilesIndexed int
	SymbolsFound int
	Errors       int
}

// Indexer scans a directory tree and builds a symbol index.
type Indexer struct {
	root    string
	workers int
	mu      sync.RWMutex
	index   map[string]*IndexEntry
}

// New creates a new Indexer rooted at the given directory.
func New(root string, workers int) *Indexer {
	return &Indexer{root: root, workers: workers, index: make(map[string]*IndexEntry)}
}

// IndexAll walks the tree indexing files with matching extensions.
func (ix *Indexer) IndexAll(exts []string) IndexStats {
	paths := ix.collectPaths(exts)
	type result struct { entry *IndexEntry; path string; err error }
	ch := make(chan result, len(paths))
	sem := make(chan struct{}, ix.workers)
	var wg sync.WaitGroup
	for _, p := range paths {
		wg.Add(1)
		go func(path string) {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()
			entry, err := ix.indexFile(path)
			ch <- result{entry: entry, path: path, err: err}
		}(p)
	}
	wg.Wait()
	close(ch)
	var stats IndexStats
	for r := range ch {
		if r.err != nil { stats.Errors++; continue }
		stats.FilesIndexed++
		stats.SymbolsFound += len(r.entry.Symbols)
		ix.mu.Lock(); ix.index[r.path] = r.entry; ix.mu.Unlock()
	}
	return stats
}

func (ix *Indexer) indexFile(path string) (*IndexEntry, error) {
	data, err := os.ReadFile(path)
	if err != nil { return nil, err }
	content := string(data)
	entry := &IndexEntry{Path: path, Lines: strings.Count(content, "\n")}
	for _, line := range strings.Split(content, "\n") {
		t := strings.TrimSpace(line)
		if strings.HasPrefix(t, "func ") || strings.HasPrefix(t, "type ") {
			parts := strings.Fields(t)
			if len(parts) >= 2 { entry.Symbols = append(entry.Symbols, parts[1]) }
		}
		if strings.HasPrefix(t, `"`) { entry.Imports = append(entry.Imports, t) }
	}
	sum := md5.Sum(data)
	entry.Checksum = hex.EncodeToString(sum[:])
	return entry, nil
}

func (ix *Indexer) collectPaths(exts []string) []string {
	var paths []string
	extSet := make(map[string]bool)
	for _, e := range exts { extSet["."+e] = true }
	filepath.WalkDir(ix.root, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() { return nil }
		if extSet[filepath.Ext(p)] { paths = append(paths, p) }
		return nil
	})
	return paths
}

// Lookup finds entries containing the given symbol.
func (ix *Indexer) Lookup(symbol string) []*IndexEntry {
	ix.mu.RLock(); defer ix.mu.RUnlock()
	var result []*IndexEntry
	for _, e := range ix.index {
		for _, s := range e.Symbols {
			if s == symbol { result = append(result, e); break }
		}
	}
	return result
}

// Invalidate removes an entry from the index.
func (ix *Indexer) Invalidate(path string) bool {
	ix.mu.Lock(); defer ix.mu.Unlock()
	_, ok := ix.index[path]; delete(ix.index, path); return ok
}

// Stats returns a summary of the current index.
func (ix *Indexer) Stats() string {
	ix.mu.RLock(); defer ix.mu.RUnlock()
	return fmt.Sprintf("entries=%d", len(ix.index))
}
