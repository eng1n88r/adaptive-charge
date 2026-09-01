PREFIX ?= /usr/local
BIN := target/release/adaptive-charge

build:
	cargo build --release

install:
	@test -f $(BIN) || { echo "run 'make build' first (as your user, not root)"; exit 1; }
	install -m755 $(BIN) $(PREFIX)/bin/adaptive-charge
	sed 's|/usr/local|$(PREFIX)|g' contrib/adaptive-charge.service > /etc/systemd/system/adaptive-charge.service
	visudo -cf contrib/sudoers-adaptive-charge
	sed 's|/usr/local|$(PREFIX)|g' contrib/sudoers-adaptive-charge > /etc/sudoers.d/adaptive-charge
	chmod 440 /etc/sudoers.d/adaptive-charge
	-systemctl disable --now thinkpad-charge-thresholds.service 2>/dev/null
	systemctl daemon-reload
	-$(PREFIX)/bin/adaptive-charge seed --write
	systemctl enable adaptive-charge.service
	systemctl restart adaptive-charge.service
	@echo
	@$(PREFIX)/bin/adaptive-charge status

uninstall:
	-systemctl disable --now adaptive-charge.service
	rm -f /etc/systemd/system/adaptive-charge.service $(PREFIX)/bin/adaptive-charge /etc/sudoers.d/adaptive-charge
	systemctl daemon-reload

.PHONY: build install uninstall
