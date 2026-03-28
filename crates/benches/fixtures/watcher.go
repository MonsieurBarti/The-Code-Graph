package watcher

import (
	"context"
	"log"
	"time"

	"github.com/fsnotify/fsnotify"
)

// ChangeKind classifies a file system event.
type ChangeKind string

const (
	ChangeCreate ChangeKind = "create"
	ChangeModify ChangeKind = "modify"
	ChangeDelete ChangeKind = "delete"
)

// FileEvent describes a single file system change.
type FileEvent struct {
	Kind      ChangeKind
	Path      string
	Timestamp time.Time
}

// Handler is called with a batch of events after debouncing.
type Handler func(events []FileEvent)

// FileWatcher watches directories for changes.
type FileWatcher struct {
	dirs       []string
	debounce   time.Duration
	watcher    *fsnotify.Watcher
	handlers   []Handler
}

// New creates a FileWatcher for the given directories.
func New(dirs []string, debounce time.Duration) (*FileWatcher, error) {
	w, err := fsnotify.NewWatcher()
	if err != nil { return nil, err }
	return &FileWatcher{dirs: dirs, debounce: debounce, watcher: w}, nil
}

// OnChange registers a handler for file events.
func (fw *FileWatcher) OnChange(h Handler) {
	fw.handlers = append(fw.handlers, h)
}

// Start begins watching. It blocks until ctx is cancelled.
func (fw *FileWatcher) Start(ctx context.Context) error {
	for _, dir := range fw.dirs {
		if err := fw.watcher.Add(dir); err != nil { return err }
	}
	var pending []FileEvent
	ticker := time.NewTicker(fw.debounce)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return fw.watcher.Close()
		case evt, ok := <-fw.watcher.Events:
			if !ok { return nil }
			pending = append(pending, FileEvent{Kind: mapOp(evt.Op), Path: evt.Name, Timestamp: time.Now()})
		case err := <-fw.watcher.Errors:
			log.Printf("watcher error: %v", err)
		case <-ticker.C:
			if len(pending) == 0 { continue }
			batch := pending; pending = nil
			for _, h := range fw.handlers { h(batch) }
		}
	}
}

func mapOp(op fsnotify.Op) ChangeKind {
	switch {
	case op&fsnotify.Create != 0: return ChangeCreate
	case op&fsnotify.Write != 0:  return ChangeModify
	case op&fsnotify.Remove != 0: return ChangeDelete
	default:                      return ChangeModify
	}
}
