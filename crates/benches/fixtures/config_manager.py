from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Optional, Union

import toml
import yaml
from pydantic import BaseModel, Field, validator


class DatabaseConfig(BaseModel):
    url: str
    pool_size: int = 5
    echo: bool = False

    @validator("pool_size")
    def pool_size_positive(cls, v: int) -> int:
        if v <= 0:
            raise ValueError("pool_size must be positive")
        return v


class ServerConfig(BaseModel):
    host: str = "0.0.0.0"
    port: int = Field(8080, ge=1, le=65535)
    workers: int = 4
    debug: bool = False


class AppConfig(BaseModel):
    server: ServerConfig = Field(default_factory=ServerConfig)
    database: DatabaseConfig
    log_level: str = "info"
    features: dict[str, bool] = Field(default_factory=dict)

    class Config:
        env_prefix = "APP_"


def load_yaml(path: Union[str, Path]) -> dict[str, Any]:
    with open(path) as f:
        return yaml.safe_load(f) or {}


def load_toml(path: Union[str, Path]) -> dict[str, Any]:
    return toml.load(str(path))


def load_config(config_path: Optional[Path] = None) -> AppConfig:
    path = config_path or Path(os.environ.get("CONFIG_PATH", "config.yaml"))
    if not path.exists():
        raise FileNotFoundError(f"Config not found: {path}")
    raw = load_yaml(path) if path.suffix in {".yaml", ".yml"} else load_toml(path)
    return AppConfig(**raw)


def merge_with_env(config: AppConfig) -> AppConfig:
    overrides: dict[str, Any] = {}
    if port := os.environ.get("PORT"):
        overrides.setdefault("server", {})["port"] = int(port)
    if db_url := os.environ.get("DATABASE_URL"):
        overrides.setdefault("database", {})["url"] = db_url
    if not overrides:
        return config
    data = config.dict()
    for k, v in overrides.items():
        if isinstance(v, dict):
            data[k].update(v)
        else:
            data[k] = v
    return AppConfig(**data)
