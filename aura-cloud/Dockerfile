# Stage 1: Build the Go binary
FROM golang:1.22-alpine AS builder

# Install protobuf compiler and build-base
RUN apk add --no-cache protobuf protobuf-dev git build-base

# Configure environment for Go plugins
ENV GO111MODULE=on
ENV CGO_ENABLED=0
ENV GOOS=linux

# Install protoc plugins for Go
RUN go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.33.0
RUN go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.3.0

WORKDIR /app

# Copy go.mod and go.sum files
COPY go.mod ./
# Note: go.sum might not exist yet if there are no external dependencies beyond std/grpc,
# but we try to copy it if it does
COPY go.sum* ./
RUN go mod download

# Copy all source files
COPY . .

# Generate the protobuf Go files from proto/aura_cloud.proto
RUN mkdir -p proto/auracloudv1 && \
    protoc --go_out=. --go-grpc_out=. proto/aura_cloud.proto

# Build the server binary
RUN go build -ldflags="-w -s" -o aura-cloud ./cmd/server

# Stage 2: Final minimal run container
FROM alpine:latest
RUN apk add --no-cache ca-certificates

WORKDIR /root/

# Copy the compiled binary from the builder stage
COPY --from=builder /app/aura-cloud .

# Expose gRPC port
EXPOSE 50051

# Run the server
CMD ["./aura-cloud"]
