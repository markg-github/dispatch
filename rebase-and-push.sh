#!/bin/bash
# rebase-and-push.sh - Rebase local branch on upstream and force push to origin

set -e  # Exit on error

# Check if we're in a git repo
if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "ERROR: Not in a git repository"
    exit 1
fi

# Get current branch name
BRANCH=$(git branch --show-current)
if [ -z "$BRANCH" ]; then
    echo "ERROR: Not on a branch (detached HEAD?)"
    exit 1
fi

echo "Working on branch: $BRANCH"
echo "Current directory: $(pwd)"
echo ""

# Fetch upstream
echo "=== Fetching upstream ==="
git fetch upstream

# Start rebase
echo ""
echo "=== Starting rebase on upstream/main ==="
git rebase upstream/main

# If rebase had conflicts, it will pause here
# User needs to resolve and run: git rebase --continue
if [ -d ".git/rebase-merge" ] || [ -d ".git/rebase-apply" ]; then
    echo ""
    echo "⚠️  REBASE PAUSED - conflicts detected"
    echo "Steps to continue:"
    echo "  1. Resolve conflicts in the files"
    echo "  2. git add <resolved-files>"
    echo "  3. git rebase --continue"
    echo "  4. Re-run this script (it will skip to squash)"
    exit 0
fi

# Show diff vs upstream
echo ""
echo "=== Diff vs upstream/main ==="
git diff upstream/main

# Ask if user wants to squash
echo ""
read -p "How many commits to squash? (0 to skip): " NUM_SQUASH

if [ "$NUM_SQUASH" -gt 0 ]; then
    echo ""
    echo "=== Squashing last $NUM_SQUASH commits ==="
    git rebase -i HEAD~"$NUM_SQUASH"
    
    # Show result
    echo ""
    echo "=== Log after squash ==="
    git log --graph -2
fi

# Show final diff
echo ""
echo "=== Final diff vs upstream/main ==="
git diff upstream/main...HEAD

# Confirm before force push
echo ""
read -p "Force push to origin/$BRANCH? (y/N): " CONFIRM
if [ "$CONFIRM" = "y" ] || [ "$CONFIRM" = "Y" ]; then
    echo ""
    echo "=== Force pushing to origin/$BRANCH ==="
    git push --force-with-lease origin "$BRANCH"
    echo "✅ Done!"
else
    echo "Push cancelled"
fi
