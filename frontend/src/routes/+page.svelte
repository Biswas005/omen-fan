<script lang="ts">
    import { onMount } from 'svelte';
    import FanCurveChart from '../FanCurveChart.svelte';

    let cpuTemp = 0;
    let gpuTemp = 0;
    let fan1 = 0;
    let fan2 = 0;
    let powerMode = 'Balanced';
    let manualFan = false;
    let fanSpeed = 50;
  
    let fanCurve = [
      { temp: 30, pwm: 20 },
      { temp: 40, pwm: 30 },
      { temp: 50, pwm: 50 },
      { temp: 60, pwm: 65 },
      { temp: 70, pwm: 80 },
      { temp: 80, pwm: 95 },
      { temp: 90, pwm: 100 }
    ];
  
    async function fetchStatus() {
      const res = await fetch('http://localhost:3000/status');
      const data = await res.json();
      cpuTemp = data.cpu_temp;
      gpuTemp = data.gpu_temp;
      fan1 = data.fan1;
      fan2 = data.fan2;
      powerMode = data.power_mode;
    }
  
    async function updatePowerMode(mode: string) {
      powerMode = mode;
      await fetch('http://localhost:3000/mode', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mode })
      });
    }
  
    async function updateFanConfig() {
      await fetch('http://localhost:3000/fan', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ manual: manualFan, speed: fanSpeed })
      });
    }
  
    function resetCurve() {
      fanCurve = [
        { temp: 30, pwm: 20 },
        { temp: 40, pwm: 30 },
        { temp: 50, pwm: 50 },
        { temp: 60, pwm: 65 },
        { temp: 70, pwm: 80 },
        { temp: 80, pwm: 95 },
        { temp: 90, pwm: 100 }
      ];
    }
  
    async function sendCurve() {
      await fetch('http://localhost:3000/fancurve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ curve: fanCurve })
      });
    }
  
    onMount(() => {
      fetchStatus();
      const interval = setInterval(fetchStatus, 2000);
      return () => clearInterval(interval);
    });
  </script>
  
  <div class="p-6 space-y-6 text-white bg-black min-h-screen">
    <h1 class="text-3xl font-bold text-center">Fan and Power Control</h1>
  
    <!-- Power Mode Buttons -->
    <div class="flex justify-center gap-4 mt-6">
      {#each ['PowerSaving', 'Balanced', 'Performance'] as mode}
        <button
          class="px-6 py-2 rounded-xl text-white font-semibold transition-all duration-200 border
                  border-gray-600 hover:border-cyan-500 hover:bg-cyan-800/30
                  bg-gradient-to-br from-gray-900 to-gray-800 shadow
                  {mode === powerMode ? 'ring-2 ring-cyan-400' : ''}"
          on:click={() => updatePowerMode(mode)}
        >
          {mode === 'PowerSaving' ? 'Eco' : mode}
        </button>
      {/each}
    </div>
  
    <!-- Manual Fan Control Toggle -->
    <div class="flex flex-col items-center gap-4 mt-6">
      <label class="inline-flex items-center cursor-pointer">
        <span class="mr-3 text-lg font-medium">Manual Fan Control</span>
        <input type="checkbox" bind:checked={manualFan} class="sr-only peer" on:change={updateFanConfig} />
        <div
          class="relative w-11 h-6 bg-gray-700 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-cyan-500/50
                rounded-full peer dark:bg-gray-600 peer-checked:after:translate-x-full
                peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px]
                after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5
                after:transition-all dark:border-gray-500 peer-checked:bg-cyan-600"
        ></div>
      </label>
  
      {#if manualFan}
        <div class="flex items-center gap-4">
          <input
            type="range"
            min="0"
            max="100"
            step="1"
            bind:value={fanSpeed}
            on:input={updateFanConfig}
            class="w-64 accent-cyan-400 bg-gray-800 rounded-lg appearance-none h-2 cursor-pointer"
          />
          <span class="text-xl font-semibold text-cyan-300">{fanSpeed}%</span>
        </div>
      {/if}
    </div>
  
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4 bg-gray-800 p-4 rounded-lg text-center">
      <div>CPU: {cpuTemp}°C</div>
      <div>GPU: {gpuTemp}°C</div>
      <div>Fan1: {fan1} RPM</div>
      <div>Fan2: {fan2} RPM</div>
    </div>
  
    <h2 class="text-2xl font-semibold mt-6">Fan Curve</h2>
    <FanCurveChart {fanCurve} />
  
    <div class="flex justify-center gap-4">
      <button on:click={sendCurve} class="bg-blue-600 px-4 py-2 rounded hover:bg-blue-500">Send Curve to Backend</button>
      <button on:click={resetCurve} class="bg-red-600 px-4 py-2 rounded hover:bg-red-500">Reset to Default</button>
    </div>
  </div>