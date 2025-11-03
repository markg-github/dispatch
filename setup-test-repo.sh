#!/bin/bash
# setup-test-repo.sh

# if they get past blocked commits, not going to stop them from pushing
# git remote set-url --push origin DISABLE_PUSH_USE_DEV_SYSTEM

cat > .git/hooks/pre-commit << 'EOF'
#!/bin/sh
echo "╔════════════════════════════════════════════╗"
echo "║  ERROR: Commits blocked on test systems!  ║"
echo "║  Make changes in dev environment          ║"
echo "╚════════════════════════════════════════════╝"
exit 1
EOF
chmod +x .git/hooks/pre-commit


echo "Test repo protection enabled!"e