FROM node:22-slim

# Install ACP adapters (Zed packages)
RUN npm install -g @zed-industries/claude-code-acp @zed-industries/codex-acp

# Add hirsel binary (build with: cargo build --release --no-default-features)
COPY hirsel /usr/local/bin/

EXPOSE 8080
CMD ["hirsel", "serve", "--port", "8080"]
