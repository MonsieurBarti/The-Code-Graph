package config

import (
	"errors"
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

// ServerConfig holds HTTP server settings.
type ServerConfig struct {
	Host    string `yaml:"host"`
	Port    int    `yaml:"port"`
	Workers int    `yaml:"workers"`
	TLS     bool   `yaml:"tls"`
}

// DatabaseConfig holds database connection settings.
type DatabaseConfig struct {
	URL            string `yaml:"url"`
	MaxConnections int    `yaml:"max_connections"`
	IdleTimeoutSec int    `yaml:"idle_timeout_sec"`
}

// AppConfig is the top-level configuration struct.
type AppConfig struct {
	Server   ServerConfig   `yaml:"server"`
	Database DatabaseConfig `yaml:"database"`
	LogLevel string         `yaml:"log_level"`
	Features map[string]bool `yaml:"features"`
}

// DefaultConfig returns a config with sensible defaults.
func DefaultConfig() *AppConfig {
	return &AppConfig{
		Server:   ServerConfig{Host: "0.0.0.0", Port: 8080, Workers: 4},
		LogLevel: "info",
		Features: make(map[string]bool),
	}
}

// LoadFile loads configuration from a YAML file.
func LoadFile(path string) (*AppConfig, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("open config: %w", err)
	}
	defer f.Close()
	cfg := DefaultConfig()
	if err := yaml.NewDecoder(f).Decode(cfg); err != nil {
		return nil, fmt.Errorf("decode config: %w", err)
	}
	return cfg, cfg.Validate()
}

// Validate checks required fields.
func (c *AppConfig) Validate() error {
	if c.Database.URL == "" {
		return errors.New("database.url is required")
	}
	if c.Server.Port <= 0 {
		return fmt.Errorf("server.port must be > 0, got %d", c.Server.Port)
	}
	return nil
}

// MergeEnv overrides config values from environment variables.
func MergeEnv(cfg *AppConfig) {
	if url := os.Getenv("DATABASE_URL"); url != "" {
		cfg.Database.URL = url
	}
	if level := os.Getenv("LOG_LEVEL"); level != "" {
		cfg.LogLevel = level
	}
}
