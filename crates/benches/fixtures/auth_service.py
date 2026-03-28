import hashlib
import hmac
import os
import time
from dataclasses import dataclass, field
from typing import Optional

import jwt
from passlib.context import CryptContext

pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")
SECRET_KEY = os.environ.get("SECRET_KEY", "dev-secret-key")
ALGORITHM = "HS256"
TOKEN_EXPIRE_SECONDS = 3600


@dataclass
class TokenPayload:
    sub: str
    exp: int
    roles: list[str] = field(default_factory=list)


@dataclass
class AuthResult:
    access_token: str
    token_type: str = "bearer"
    expires_in: int = TOKEN_EXPIRE_SECONDS


def hash_password(plain: str) -> str:
    return pwd_context.hash(plain)


def verify_password(plain: str, hashed: str) -> bool:
    return pwd_context.verify(plain, hashed)


def create_access_token(subject: str, roles: list[str]) -> str:
    payload = {
        "sub": subject,
        "roles": roles,
        "exp": int(time.time()) + TOKEN_EXPIRE_SECONDS,
        "iat": int(time.time()),
    }
    return jwt.encode(payload, SECRET_KEY, algorithm=ALGORITHM)


def decode_token(token: str) -> Optional[TokenPayload]:
    try:
        data = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        return TokenPayload(sub=data["sub"], exp=data["exp"], roles=data.get("roles", []))
    except jwt.PyJWTError:
        return None


def generate_api_key(prefix: str = "cg") -> str:
    raw = os.urandom(32)
    digest = hmac.new(SECRET_KEY.encode(), raw, hashlib.sha256).hexdigest()
    return f"{prefix}_{digest[:40]}"
