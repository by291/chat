.DEFAULT_GOAL := help

PROCESS_COMPOSE ?= process-compose
ENV_FILE ?= .env
DEV_DIR ?= .local/dev
DEV_CONFIG ?= process-compose.yaml
COMPOSE_PROJECT_NAME ?= chat-dev
export CHAT_DEV_DIR := $(abspath $(DEV_DIR))
# Keep optional supervisor settings local instead of probing missing user directories.
export PROC_COMP_CONFIG ?= $(CHAT_DEV_DIR)

# A private per-checkout socket keeps stop/status scoped to this supervisor.
PC = $(PROCESS_COMPOSE) --unix-socket "$(CHAT_DEV_DIR)/process-compose.sock" --log-file "$(CHAT_DEV_DIR)/supervisor.log"
COMPOSE = docker compose --project-name "$(COMPOSE_PROJECT_NAME)" --env-file "$(ENV_FILE)"

.PHONY: help dev-init infra infra-stop dev dev-stop dev-status dev-logs

help:
	@printf '%s\n' \
	  'make dev-init    Create development credentials once' \
	  'make infra       Start Docker dependencies' \
	  'make infra-stop  Stop Docker dependencies, preserving data' \
	  'make dev         Build and start API, worker and web in the background' \
	  'make dev-stop    Stop application processes only' \
	  'make dev-status  Show startup progress and process health' \
	  'make dev-logs    Follow application and setup logs'

dev-init:
	node scripts/init-env.mjs

infra:
	$(COMPOSE) up -d --wait

infra-stop:
	$(COMPOSE) down

dev:
	@command -v "$(PROCESS_COMPOSE)" >/dev/null || { printf '%s\n' 'Process Compose is missing. See docs/operations/development.md for setup; no tools were installed.'; exit 1; }
	@test -f "$(ENV_FILE)" || { printf '%s\n' 'Missing environment file. Run make dev-init first.'; exit 1; }
	@node scripts/check-dev-infra.mjs --project-name "$(COMPOSE_PROJECT_NAME)" --env-file "$(ENV_FILE)"
	@mkdir -p "$(CHAT_DEV_DIR)"
	@chmod 700 "$(CHAT_DEV_DIR)"
	@if $(PC) process list >/dev/null 2>&1; then \
	  printf '%s\n' 'Development processes are already running. Use make dev-status or make dev-stop.'; \
	else \
	  $(PC) --config "$(DEV_CONFIG)" --env "$(abspath $(ENV_FILE))" --ordered-shutdown up --detached && \
	  printf '%s\n' 'Starting in the background. Use make dev-status or make dev-logs; open http://localhost:5173 when ready.'; \
	fi

dev-stop:
	@$(PC) down

dev-status:
	@$(PC) process list --output wide

dev-logs:
	@tail -F "$(CHAT_DEV_DIR)/processes.log"
