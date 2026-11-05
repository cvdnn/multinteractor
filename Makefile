# multinteractor — 统一构建、检查与测试入口

PYTHON ?= /usr/bin/python3

.PHONY: help server-dev server-test client-check client-build client-test test-all task-status clean

help:
	@echo "Available targets:"
	@echo "  server-dev     启动云端服务端"
	@echo "  server-test    运行服务端单元测试"
	@echo "  client-check   检查客户端 Cargo Workspace 编译"
	@echo "  client-build   编译客户端守护进程与 mi CLI 工具"
	@echo "  client-test    运行客户端单元测试"
	@echo "  test-all       运行 L1~L4 全量单元、集成、混沌与端到端验收测试"
	@echo "  task-status    查看任务原子锁状态 (.task_lock.json)"
	@echo "  clean          清理构建产物与临时缓存文件"

server-dev:
	cd server && $(PYTHON) -m uvicorn app.main:app --reload --host 0.0.0.0 --port 8000

server-test:
	$(PYTHON) -m unittest discover -s server/tests -p "test_*.py" -v

client-check:
	cd client && cargo check --workspace

client-build:
	cd client && cargo build --workspace

client-test:
	cd client && cargo test --workspace

test-all:
	$(PYTHON) -m unittest \
		tests/fixtures/test_protocol_contract.py \
		server/tests/test_client_sub_manager.py \
		server/tests/test_audit_logger.py \
		server/tests/test_stream_tee_cache.py \
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
