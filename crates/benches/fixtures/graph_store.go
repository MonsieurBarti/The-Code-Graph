package graph

import (
	"database/sql"
	"errors"
	"fmt"

	_ "github.com/mattn/go-sqlite3"
)

// Node represents a code symbol node.
type Node struct {
	ID       string
	Kind     string
	Label    string
	FilePath string
	Line     int
}

// Edge represents a directed relationship between nodes.
type Edge struct {
	Source string
	Target string
	Kind   string
}

// GraphStore persists nodes and edges in SQLite.
type GraphStore struct {
	db          *sql.DB
	adjacency   map[string][]string
	reverseAdj  map[string][]string
}

// Open opens (or creates) a SQLite-backed graph store.
func Open(path string) (*GraphStore, error) {
	db, err := sql.Open("sqlite3", path)
	if err != nil {
		return nil, fmt.Errorf("open db: %w", err)
	}
	s := &GraphStore{db: db, adjacency: make(map[string][]string), reverseAdj: make(map[string][]string)}
	return s, s.initSchema()
}

func (s *GraphStore) initSchema() error {
	_, err := s.db.Exec(`
		CREATE TABLE IF NOT EXISTS nodes (id TEXT PRIMARY KEY, kind TEXT, label TEXT, file_path TEXT, line INTEGER);
		CREATE TABLE IF NOT EXISTS edges (source TEXT, target TEXT, kind TEXT, PRIMARY KEY(source, target, kind));
	`)
	return err
}

// InsertNode upserts a node.
func (s *GraphStore) InsertNode(n *Node) error {
	_, err := s.db.Exec("INSERT OR REPLACE INTO nodes VALUES (?,?,?,?,?)", n.ID, n.Kind, n.Label, n.FilePath, n.Line)
	return err
}

// InsertEdge upserts an edge and updates in-memory adjacency.
func (s *GraphStore) InsertEdge(e *Edge) error {
	_, err := s.db.Exec("INSERT OR REPLACE INTO edges VALUES (?,?,?)", e.Source, e.Target, e.Kind)
	if err != nil {
		return err
	}
	s.adjacency[e.Source] = append(s.adjacency[e.Source], e.Target)
	s.reverseAdj[e.Target] = append(s.reverseAdj[e.Target], e.Source)
	return nil
}

// GetNode retrieves a node by ID.
func (s *GraphStore) GetNode(id string) (*Node, error) {
	row := s.db.QueryRow("SELECT id,kind,label,file_path,line FROM nodes WHERE id=?", id)
	n := &Node{}
	if err := row.Scan(&n.ID, &n.Kind, &n.Label, &n.FilePath, &n.Line); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil, nil
		}
		return nil, err
	}
	return n, nil
}

// Close closes the database.
func (s *GraphStore) Close() error { return s.db.Close() }
