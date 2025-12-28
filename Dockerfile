# Build stage
FROM rust:1.83 as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY index.html ./

# Build for release
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/target/release/rust-messaging-app .
COPY --from=builder /app/index.html .

# Expose port
EXPOSE 8080

# Set environment variable for production
ENV RUST_LOG=info

# Run the application
CMD ["./rust-messaging-app"]
