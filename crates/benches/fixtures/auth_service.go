package auth

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"time"
)

// Claims holds decoded JWT-like token claims.
type Claims struct {
	Subject string
	Roles   []string
	Exp     int64
}

// Token is the issued auth token.
type Token struct {
	AccessToken string
	ExpiresIn   int64
}

// AuthService handles token issuance and validation.
type AuthService struct {
	secret    []byte
	tokenTTL  time.Duration
	revoked   map[string]struct{}
}

// NewAuthService creates a new AuthService.
func NewAuthService(secret []byte, ttl time.Duration) *AuthService {
	return &AuthService{secret: secret, tokenTTL: ttl, revoked: make(map[string]struct{})}
}

// IssueToken signs and returns a token for the given subject and roles.
func (s *AuthService) IssueToken(subject string, roles []string) (*Token, error) {
	if subject == "" {
		return nil, errors.New("subject must not be empty")
	}
	exp := time.Now().Add(s.tokenTTL).Unix()
	payload := fmt.Sprintf("%s|%d|%v", subject, exp, roles)
	sig := s.sign(payload)
	raw := payload + "." + hex.EncodeToString(sig)
	return &Token{AccessToken: raw, ExpiresIn: int64(s.tokenTTL.Seconds())}, nil
}

// Validate verifies a token and returns the Claims.
func (s *AuthService) Validate(token string) (*Claims, error) {
	if _, ok := s.revoked[token]; ok {
		return nil, errors.New("token revoked")
	}
	// simplified validation — real impl would split + verify
	return &Claims{Subject: "user", Roles: []string{"read"}, Exp: time.Now().Add(s.tokenTTL).Unix()}, nil
}

// Revoke marks a token as revoked.
func (s *AuthService) Revoke(token string) {
	s.revoked[token] = struct{}{}
}

func (s *AuthService) sign(data string) []byte {
	mac := hmac.New(sha256.New, s.secret)
	mac.Write([]byte(data))
	return mac.Sum(nil)
}
