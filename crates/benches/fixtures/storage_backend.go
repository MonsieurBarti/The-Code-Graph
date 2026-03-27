package storage

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"

	_ "github.com/mattn/go-sqlite3"
)

// ErrNotFound is returned when a key is absent.
var ErrNotFound = errors.New("key not found")

// StorageBackend is a JSON-over-SQLite key-value store.
type StorageBackend struct {
	db *sql.DB
}

// Open opens (or creates) a storage backend at the given path.
func Open(path string) (*StorageBackend, error) {
	db, err := sql.Open("sqlite3", path+"?_journal=WAL&_synchronous=NORMAL")
	if err != nil { return nil, fmt.Errorf("open: %w", err) }
	if _, err = db.Exec("CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL)"); err != nil {
		return nil, fmt.Errorf("init schema: %w", err)
	}
	return &StorageBackend{db: db}, nil
}

// Set stores a JSON-serialisable value under key.
func (s *StorageBackend) Set(key string, value interface{}) error {
	data, err := json.Marshal(value)
	if err != nil { return fmt.Errorf("marshal: %w", err) }
	_, err = s.db.Exec("INSERT OR REPLACE INTO kv VALUES (?, ?)", key, string(data))
	return err
}

// Get retrieves and unmarshals a value by key.
func (s *StorageBackend) Get(key string, dest interface{}) error {
	row := s.db.QueryRow("SELECT value FROM kv WHERE key = ?", key)
	var raw string
	if err := row.Scan(&raw); err != nil {
		if errors.Is(err, sql.ErrNoRows) { return ErrNotFound }
		return err
	}
	return json.Unmarshal([]byte(raw), dest)
}

// Delete removes a key.
func (s *StorageBackend) Delete(key string) (bool, error) {
	res, err := s.db.Exec("DELETE FROM kv WHERE key = ?", key)
	if err != nil { return false, err }
	n, _ := res.RowsAffected()
	return n > 0, nil
}

// Count returns the total number of keys.
func (s *StorageBackend) Count() (int64, error) {
	var n int64
	return n, s.db.QueryRow("SELECT COUNT(*) FROM kv").Scan(&n)
}

// Close closes the underlying database.
func (s *StorageBackend) Close() error { return s.db.Close() }
