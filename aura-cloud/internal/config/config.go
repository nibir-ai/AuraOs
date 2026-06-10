package config

import (
	"os"
	"strconv"
)

type Config struct {
	Port          string
	GeminiAPIKey  string
	RedisURL      string
	FreeTierLimit int
}

func LoadConfig() *Config {
	port := os.Getenv("PORT")
	if port == "" {
		port = "50051"
	}

	geminiKey := os.Getenv("GEMINI_API_KEY")
	if geminiKey == "" {
		geminiKey = "MOCK_GEMINI_API_KEY" // Fallback for dev
	}

	redisURL := os.Getenv("RATE_LIMIT_REDIS_URL")
	if redisURL == "" {
		redisURL = "localhost:6379" // Fallback for dev
	}

	freeLimit := 50
	if limitStr := os.Getenv("FREE_TIER_LIMIT"); limitStr != "" {
		if limit, err := strconv.Atoi(limitStr); err == nil {
			freeLimit = limit
		}
	}

	return &Config{
		Port:          port,
		GeminiAPIKey:  geminiKey,
		RedisURL:      redisURL,
		FreeTierLimit: freeLimit,
	}
}
