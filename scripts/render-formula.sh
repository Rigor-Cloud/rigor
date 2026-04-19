#!/usr/bin/env bash
# Emits Formula/rigor.rb for Rigor-Cloud/homebrew-rigor with the given
# version and per-architecture SHA256s. Called by .github/workflows/release.yml.
#
# Usage: render-formula.sh <version> <sha_arm64> <sha_x86_64>

set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <sha_arm64> <sha_x86_64>" >&2
  exit 2
fi

VERSION="$1"
SHA_ARM="$2"
SHA_X64="$3"

cat <<EOF
class Rigor < Formula
  desc "Epistemic guardrails, knowledge graph, and metacognition for AI"
  homepage "https://rigorcloud.com"
  version "${VERSION}"
  license :cannot_represent

  on_macos do
    on_arm do
      url "https://github.com/Rigor-Cloud/rigor-releases/releases/download/v${VERSION}/rigor-${VERSION}-aarch64-apple-darwin.tar.gz"
      sha256 "${SHA_ARM}"
    end
    on_intel do
      url "https://github.com/Rigor-Cloud/rigor-releases/releases/download/v${VERSION}/rigor-${VERSION}-x86_64-apple-darwin.tar.gz"
      sha256 "${SHA_X64}"
    end
  end

  def install
    bin.install "rigor"
  end

  test do
    assert_match "Epistemic constraint enforcement", shell_output("#{bin}/rigor --help")
  end
end
EOF
