package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/auraos/aura-cloud/internal/auth"
	"github.com/auraos/aura-cloud/internal/config"
	"github.com/auraos/aura-cloud/internal/gemini"
	"github.com/auraos/aura-cloud/internal/ratelimit"
	pb "github.com/auraos/aura-cloud/proto/auracloudv1"
)

type server struct {
	pb.UnimplementedGeminiProxyServer
	cfg            *config.Config
	tokenValidator *auth.TokenValidator
	limiter        *ratelimit.Limiter
	geminiProxy    *gemini.Proxy
}

func newServer(cfg *config.Config) *server {
	return &server{
		cfg:            cfg,
		tokenValidator: auth.NewTokenValidator(),
		limiter:        ratelimit.NewLimiter(cfg.RedisURL),
		geminiProxy:    gemini.NewProxy(cfg.GeminiAPIKey),
	}
}

func (s *server) StreamQuery(req *pb.QueryRequest, stream pb.GeminiProxy_StreamQueryServer) error {
	ctx := stream.Context()

	// 1. Authenticate user's Google Access Token
	tokenInfo, err := s.tokenValidator.ValidateToken(ctx, req.GoogleAccessToken)
	if err != nil {
		log.Printf("Authentication failed: %v", err)
		return status.Errorf(codes.Unauthenticated, "invalid Google access token: %v", err)
	}

	// 2. Determine User Tier (free by default; email ending in @auraos.io or containing "premium" gets premium)
	tier := pb.UserTier_USER_TIER_FREE
	limit := s.cfg.FreeTierLimit
	model := "gemini-2.0-flash" // Default fast model for free tier

	if strings.Contains(tokenInfo.Email, "premium") || strings.HasSuffix(tokenInfo.Email, "@auraos.io") {
		tier = pb.UserTier_USER_TIER_PREMIUM
		limit = 0 // No rate limit
		model = "gemini-2.5-pro" // Upgrade premium users to pro model
	}

	// 3. Enforce Rate Limiting
	allowed, err := s.limiter.Allow(ctx, tokenInfo.Sub, limit)
	if err != nil {
		log.Printf("Rate limiter error: %v", err)
		// Fall through on rate limit check error to avoid locking out users on Redis transient failure
	} else if !allowed {
		log.Printf("Rate limit exceeded for user %s (%s)", tokenInfo.Sub, tokenInfo.Email)
		return status.Errorf(codes.ResourceExhausted, "daily rate limit of %d queries exceeded for tier %s", limit, tier)
	}

	// 4. Proxy Query and stream content
	log.Printf("Processing query from %s (tier=%s, model=%s)", tokenInfo.Email, tier, model)
	err = s.geminiProxy.StreamQuery(ctx, req, model, func(chunk *pb.QueryChunk) error {
		return stream.Send(chunk)
	})
	if err != nil {
		log.Printf("Proxy error: %v", err)
		return status.Errorf(codes.Internal, "error communicating with Gemini API: %v", err)
	}

	return nil
}

func (s *server) ValidateAccount(ctx context.Context, req *pb.ValidateRequest) (*pb.ValidateResponse, error) {
	// 1. Validate Google Access Token
	tokenInfo, err := s.tokenValidator.ValidateToken(ctx, req.GoogleAccessToken)
	if err != nil {
		return &pb.ValidateResponse{
			Valid:        false,
			ErrorMessage: fmt.Sprintf("invalid access token: %v", err),
		}, nil
	}

	// 2. Fetch detailed profile info
	profile, err := s.tokenValidator.FetchUserProfile(ctx, req.GoogleAccessToken)
	if err != nil {
		// Fallback to token info values if full profile fetch fails
		profile = &auth.UserProfile{
			Sub:         tokenInfo.Sub,
			Email:       tokenInfo.Email,
			DisplayName: strings.Split(tokenInfo.Email, "@")[0],
		}
	}

	// 3. Determine Tier
	tier := pb.UserTier_USER_TIER_FREE
	if strings.Contains(profile.Email, "premium") || strings.HasSuffix(profile.Email, "@auraos.io") {
		tier = pb.UserTier_USER_TIER_PREMIUM
	}

	log.Printf("Validated account: %s (tier=%s)", profile.Email, tier)

	return &pb.ValidateResponse{
		Valid:       true,
		GoogleSub:   profile.Sub,
		Email:       profile.Email,
		DisplayName: profile.DisplayName,
		Tier:        tier,
	}, nil
}

func main() {
	cfg := config.LoadConfig()

	lis, err := net.Listen("tcp", ":"+cfg.Port)
	if err != nil {
		log.Fatalf("failed to listen on port %s: %v", cfg.Port, err)
	}

	s := grpc.NewServer()
	pb.RegisterGeminiProxyServer(s, newServer(cfg))

	log.Printf("AuraOS Cloud Backend running on port %s...", cfg.Port)
	if err := s.Serve(lis); err != nil {
		log.Fatalf("failed to serve gRPC: %v", err)
	}
}
