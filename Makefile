# multinteractor — 统一构建、检查与测试入口

PYTHON ?= $(if $(wildcard $(CURDIR)/.venv/bin/python),$(CURDIR)/.venv/bin/python,$(shell command -v python3 2>/dev/null || echo /usr/bin/python3))
NPM ?= $(shell command -v npm 2>/dev/null || echo npm)
CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)

.PHONY: help server-dev server-test webui-install webui-typecheck webui-build client-check client-build client-test package-macos-app test-all task-status clean

help:
	@echo "Available targets:"
	@echo "  server-dev       启动云端服务端"
	@echo "  server-test      运行服务端单元测试"
	@echo "  webui-install    安装 WebUI (TypeScript + Vite) 开发依赖"
	@echo "  webui-typecheck  执行 WebUI TypeScript 类型检查"
	@echo "  webui-build      编译 WebUI 至 server/app/static/ (零 CDN 离线产物)"
	@echo "  client-check     检查客户端 Cargo Workspace 编译"
	@echo "  client-build     编译客户端守护进程与 mi CLI 工具"
	@echo "  client-test      运行客户端单元测试"
	@echo "  package-macos-app 打包生成 macOS Multinteractor.app 桌面应用"
	@echo "  test-all         运行 L1~L4 全量单元、集成、混沌与端到端验收测试"
	@echo "  task-status      查看任务原子锁状态 (.task_lock.json)"
	@echo "  clean            清理构建产物与临时缓存文件"

server-dev:
	cd server && $(PYTHON) -m uvicorn app.main:app --reload --host 0.0.0.0 --port 8000

server-test:
	$(PYTHON) -m unittest discover -s server/tests -p "test_*.py" -v

webui-install:
	cd server/webui && $(NPM) install

webui-typecheck:
	cd server/webui && $(NPM) run typecheck

webui-build:
	cd server/webui && $(NPM) run build

client-check:
	cd client && $(CARGO) check --workspace

client-build:
	cd client && $(CARGO) build --workspace

client-test:
	cd client && $(CARGO) test --workspace

version:
	@cat VERSION

bump-version:
	@cur=$$(tr -d '[:space:]' < VERSION); \
	num=$${cur#v}; \
	next="v$$((num + 1))"; \
	echo "$$next" > VERSION; \
	echo "Updated VERSION: $$cur -> $$next"

package-macos-app:
	bash deploy/client/macos/package_macos_app.sh

package-macos-app-bump:
	bash deploy/client/macos/package_macos_app.sh --bump

test-all:
	$(PYTHON) -m unittest \
		tests/fixtures/test_protocol_contract.py \
		server/tests/test_client_sub_manager.py \
		server/tests/test_audit_logger.py \
		server/tests/test_stream_tee_cache.py \
		server/tests/test_webui_api.py \
		tests/integration/test_signaling_and_audit.py \
		tests/integration/test_stream_teeing_pipeline.py \
		tests/chaos/test_resumable_transfer.py \
		tests/chaos/test_lan_idle_disconnect.py \
		tests/e2e/test_cli_and_skill_workflow.py \
		tests/e2e/test_clipboard_and_sync_e2e.py \
		-v

task-status:
	$(PYTHON) scripts/task_guard.py status

clean:
	find . -type d -name "__pycache__" -exec rm -rf {} +
	find . -type d -name ".pytest_cache" -exec rm -rf {} +
	find . -type f -name "*.multinteractor.part" -delete
	find . -type f -name "*.multinteractor.resume.json" -delete
