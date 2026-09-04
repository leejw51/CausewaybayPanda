/* EIP-1193 wallet, no library. The server never holds a key: it prices the
   cart and hands over calldata, the guest's own wallet signs and broadcasts. */
(function (global) {
  function provider() {
    return global.ethereum || null;
  }

  const PandaWallet = {
    available() {
      return Boolean(provider());
    },

    /** Ask for an account. Returns the address, or throws with a plain reason. */
    async connect() {
      const eth = provider();
      if (!eth) throw new Error("no wallet in this browser");
      const accounts = await eth.request({ method: "eth_requestAccounts" });
      if (!accounts || !accounts.length) throw new Error("no account was shared");
      return accounts[0];
    },

    /** Point the wallet at the cafe's chain, adding it if it is unknown. */
    async ensureChain(chain) {
      const eth = provider();
      const want = chain.chain_id_hex;
      const now = await eth.request({ method: "eth_chainId" });
      if (String(now).toLowerCase() === want.toLowerCase()) return;
      try {
        await eth.request({
          method: "wallet_switchEthereumChain",
          params: [{ chainId: want }],
        });
      } catch (err) {
        // 4902: the wallet has never heard of this chain. Offer to add it.
        const code = err && (err.code ?? err.data?.originalError?.code);
        if (code !== 4902) throw err;
        await eth.request({
          method: "wallet_addEthereumChain",
          params: [
            {
              chainId: want,
              chainName: chain.chain_name,
              nativeCurrency: {
                name: chain.native_symbol,
                symbol: chain.native_symbol,
                decimals: chain.native_decimals,
              },
              rpcUrls: [chain.rpc_url],
              blockExplorerUrls: [chain.explorer_tx.replace(/\/tx\/?$/, "")],
            },
          ],
        });
      }
    },

    /** Send the prepared ERC-20 transfer. Returns the transaction hash. */
    async send({ from, to, data }) {
      const eth = provider();
      const hash = await eth.request({
        method: "eth_sendTransaction",
        params: [{ from, to, data, value: "0x0" }],
      });
      if (typeof hash !== "string" || !hash.startsWith("0x")) {
        throw new Error("the wallet returned no transaction hash");
      }
      return hash;
    },

    /** Connect, switch, sign, broadcast. One call for the whole settlement. */
    async pay(chain, request) {
      const from = await this.connect();
      await this.ensureChain(chain);
      return this.send({ from, to: request.token, data: request.call_data });
    },

    /** Wallet errors are objects; pull something a guest can read out of them. */
    reason(err) {
      const code = err && (err.code ?? err.data?.originalError?.code);
      if (code === 4001) return "you turned the payment down";
      const msg = (err && (err.message || err.reason)) || String(err);
      return msg.replace(/^Error:\s*/, "");
    },
  };

  global.PandaWallet = PandaWallet;
})(window);
