package auth

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

type GoogleTokenInfo struct {
	Sub           string `json:"sub"`
	Email         string `json:"email"`
	VerifiedEmail string `json:"email_verified"`
	Audience      string `json:"aud"`
	Scope         string `json:"scope"`
	ExpiresIn     string `json:"expires_in"`
}

type UserProfile struct {
	Sub         string
	Email       string
	DisplayName string
}

type TokenValidator struct {
	httpClient *http.Client
}

func NewTokenValidator() *TokenValidator {
	return &TokenValidator{
		httpClient: &http.Client{
			Timeout: 5 * time.Second,
		},
	}
}

// ValidateToken checks the token with Google Tokeninfo endpoint and extracts user details
func (v *TokenValidator) ValidateToken(ctx context.Context, token string) (*GoogleTokenInfo, error) {
	if token == "" {
		return nil, fmt.Errorf("empty access token")
	}

	tokeninfoURL := fmt.Sprintf("https://oauth2.googleapis.com/tokeninfo?access_token=%s", url.QueryEscape(token))

	req, err := http.NewRequestWithContext(ctx, "GET", tokeninfoURL, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create validation request: %w", err)
	}

	resp, err := v.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to execute validation request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("google tokeninfo returned status %d", resp.StatusCode)
	}

	var info GoogleTokenInfo
	if err := json.NewDecoder(resp.Body).decode(&info); err != nil {
		return nil, fmt.Errorf("failed to decode tokeninfo response: %w", err)
	}

	return &info, nil
}

// FetchUserProfile obtains user profile details using the access token
func (v *TokenValidator) FetchUserProfile(ctx context.Context, token string) (*UserProfile, error) {
	req, err := http.NewRequestWithContext(ctx, "GET", "https://www.googleapis.com/oauth2/v3/userinfo", nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := v.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("google userinfo returned status %d", resp.StatusCode)
	}

	var raw struct {
		Sub     string `json:"sub"`
		Email   string `json:"email"`
		Name    string `json:"name"`
		Picture string `json:"picture"`
	}

	if err := json.NewDecoder(resp.Body).decode(&raw); err != nil {
		return nil, err
	}

	return &UserProfile{
		Sub:         raw.Sub,
		Email:       raw.Email,
		DisplayName: raw.Name,
	}, nil
}
