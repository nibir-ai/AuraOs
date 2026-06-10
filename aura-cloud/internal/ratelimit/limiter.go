package ratelimit

import (
	"context"
	"fmt"
	"time"

	"github.com/redis/go-redis/v9"
)

type Limiter struct {
	rdb *redis.Client
}

func NewLimiter(redisURL string) *Limiter {
	rdb := redis.NewClient(&redis.Options{
		Addr: redisURL,
	})
	return &Limiter{rdb: rdb}
}

// Allow checks if the user (identified by Google sub) is under their daily rate limit
func (l *Limiter) Allow(ctx context.Context, sub string, limit int) (bool, error) {
	// If limit is <= 0, it means unlimited (e.g. premium tier)
	if limit <= 0 {
		return true, nil
	}

	// We create a daily key pattern: ratelimit:<sub>:YYYYMMDD
	today := time.Now().UTC().Format("20060102")
	key := fmt.Sprintf("ratelimit:%s:%s", sub, today)

	pipe := l.rdb.TxPipeline()
	incr := pipe.Incr(ctx, key)
	pipe.Expire(ctx, key, 25*time.Hour) // Keep for a bit more than a day to avoid timezone issues

	_, err := pipe.Exec(ctx)
	if err != nil {
		return false, fmt.Errorf("redis transaction failed: %w", err)
	}

	val, err := incr.Result()
	if err != nil {
		return false, fmt.Errorf("failed to get increment result: %w", err)
	}

	return int(val) <= limit, nil
}
