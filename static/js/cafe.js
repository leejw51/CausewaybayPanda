/* Causewaybay Coffee booth — Three.js window onto the street. */
(function (global) {
  const PandaCafe = {
    renderer: null,
    scene: null,
    camera: null,
    moodName: "idle",
    lights: [],
    start(canvas) {
      if (!global.THREE || !canvas) return;
      const THREE = global.THREE;
      const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
      renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 80);
      camera.position.set(0, 1.2, 4.2);

      const ambient = new THREE.AmbientLight(0x3a2218, 0.7);
      const warm = new THREE.PointLight(0xffc07a, 1.4, 12);
      warm.position.set(-1.2, 2.2, 1.4);
      const neon = new THREE.PointLight(0xe23c8a, 0.5, 10);
      neon.position.set(1.6, 1.4, 0.4);
      scene.add(ambient, warm, neon);

      const loader = new THREE.TextureLoader();
      loader.load("/assets/cafe_interior.png", (tex) => {
        tex.colorSpace = THREE.SRGBColorSpace;
        scene.background = tex;
      });
      loader.load("/assets/zkp/street.png", (tex) => {
        tex.colorSpace = THREE.SRGBColorSpace;
        const win = new THREE.Mesh(
          new THREE.PlaneGeometry(3.6, 2.1),
          new THREE.MeshBasicMaterial({ map: tex })
        );
        win.position.set(0, 1.35, -2.4);
        scene.add(win);
      });

      const table = new THREE.Mesh(
        new THREE.BoxGeometry(2.4, 0.12, 1.1),
        new THREE.MeshStandardMaterial({ color: 0x3a2418, roughness: 0.7 })
      );
      table.position.set(0, 0.35, 0.6);
      scene.add(table);

      this.renderer = renderer;
      this.scene = scene;
      this.camera = camera;
      this.lights = [warm, neon];
      const resize = () => {
        const w = canvas.clientWidth || innerWidth;
        const h = canvas.clientHeight || innerHeight;
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
      };
      resize();
      addEventListener("resize", resize);
      const tick = (t) => {
        requestAnimationFrame(tick);
        camera.position.x = Math.sin(t * 0.00015) * 0.35;
        camera.lookAt(0, 1.1, -1);
        renderer.render(scene, camera);
      };
      requestAnimationFrame(tick);
    },
    mood(name) {
      this.moodName = name;
      const warm = this.lights[0];
      if (!warm) return;
      if (name === "paid") warm.intensity = 2.4;
      else if (name === "ordering") warm.intensity = 1.8;
      else warm.intensity = 1.4;
    },
  };
  global.PandaCafe = PandaCafe;
})(window);
