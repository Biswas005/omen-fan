<script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { Chart, registerables } from 'chart.js';
    import dragDataPlugin from 'chartjs-plugin-dragdata';
  
    Chart.register(...registerables, dragDataPlugin);
  
    export let fanCurve: { temp: number; pwm: number }[] = [];
    export let manualTuning: { temp: number; pwm: number } | null = null;
    
    // Define curve presets
    const presets = {
      silent: [
        { temp: 30, pwm: 0 },
        { temp: 45, pwm: 15 },
        { temp: 60, pwm: 30 },
        { temp: 75, pwm: 50 },
        { temp: 90, pwm: 70 },
        { temp: 100, pwm: 90 }
      ],
      balanced: [
        { temp: 30, pwm: 20 },
        { temp: 50, pwm: 30 },
        { temp: 65, pwm: 50 },
        { temp: 75, pwm: 70 },
        { temp: 85, pwm: 90 },
        { temp: 100, pwm: 100 }
      ],
      performance: [
        { temp: 30, pwm: 40 },
        { temp: 45, pwm: 60 },
        { temp: 60, pwm: 80 },
        { temp: 70, pwm: 90 },
        { temp: 80, pwm: 100 },
        { temp: 100, pwm: 100 }
      ],
      custom: [] as { temp: number; pwm: number }[] // Will be populated with current curve
    };
  
    const dispatch = createEventDispatcher();
  
    let canvasEl: HTMLCanvasElement;
    let chart: Chart | null = null;
    let selectedPreset = 'custom';
    
    // Function to sync manual tuning with chart
    function syncManualTuning() {
      if (manualTuning && chart) {
        // Find the nearest temperature point in the curve
        const nearestPoint = findNearestPoint(manualTuning.temp);
        if (nearestPoint !== -1) {
          // Update the PWM value
          (chart.data.datasets[0].data[nearestPoint] as any).y = manualTuning.pwm;
          chart.update();
          
          // Update the fanCurve array
          const updated = chart.data.datasets[0].data.map(p => ({
            temp: (p as any).x,
            pwm: (p as any).y
          }));
          dispatch('update', updated);
        }
      }
    }
    
    // Find the nearest point to a given temperature
    function findNearestPoint(temp: number): number {
      if (!chart) return -1;
      
      let nearestIndex = -1;
      let minDistance = Infinity;
      
      chart.data.datasets[0].data.forEach((point, index) => {
        const distance = Math.abs((point as any).x - temp);
        if (distance < minDistance) {
          minDistance = distance;
          nearestIndex = index;
        }
      });
      
      return nearestIndex;
    }
    
    // Apply a preset curve
    function applyPreset(presetName: string) {
      if (presetName === 'custom' && chart) {
        // Don't do anything when selecting custom if already using custom
        return;
      }
      
      selectedPreset = presetName;
      
      if (chart) {
        // Save current curve as custom before applying new preset
        if (presetName !== 'custom') {
          presets.custom = chart.data.datasets[0].data.map(p => ({
            temp: (p as any).x,
            pwm: (p as any).y
          }));
        }
        
        // Apply selected preset
        const newCurve = presets[presetName as keyof typeof presets];
        
        // Update chart data
        chart.data.labels = newCurve.map(p => `${p.temp}°C`);
        chart.data.datasets[0].data = newCurve.map(p => ({ x: p.temp, y: p.pwm }));
        chart.update();
        
        // Dispatch update event
        dispatch('update', newCurve);
      }
    }
    
    // Watch for changes in manualTuning
    $: if (manualTuning && chart) {
      syncManualTuning();
    }
  
    onMount(() => {
      const ctx = canvasEl.getContext('2d')!;
      
      // Save initial curve as custom preset
      presets.custom = [...fanCurve];
  
      chart = new Chart(ctx as CanvasRenderingContext2D, {
        type: 'line',
        data: {
          labels: fanCurve.map((p: { temp: number; pwm: number }) => `${p.temp}°C`),
          datasets: [
            {
              label: 'Fan Curve',
              data: fanCurve.map((p: { temp: number; pwm: number }) => ({ x: p.temp, y: p.pwm })),
              borderColor: '#00f2ff',
              backgroundColor: 'rgba(0,242,255,0.1)',
              pointBackgroundColor: '#00f2ff',
              pointBorderColor: '#fff',
              pointRadius: 6,
              pointHoverRadius: 8,
              tension: 0.3,
              fill: true,
            }
          ]
        },
        options: {
          animation: false,
          responsive: true,
          maintainAspectRatio: false,
          scales: {
            x: {
              type: 'linear',
              min: 30,
              max: 100,
              title: {
                display: true,
                text: 'Temperature (°C)',
                color: '#ccc',
                font: { weight: 'bold' }
              },
              ticks: {
                color: '#aaa'
              },
              grid: {
                color: '#333'
              }
            },
            y: {
              min: 0,
              max: 100,
              title: {
                display: true,
                text: 'Fan Speed (%)',
                color: '#ccc',
                font: { weight: 'bold' }
              },
              ticks: {
                color: '#aaa'
              },
              grid: {
                color: '#333'
              }
            }
          },
          plugins: {
            legend: {
              display: false
            },
            tooltip: {
              enabled: true,
              backgroundColor: '#222',
              titleColor: '#0ff',
              bodyColor: '#fff'
            },
            dragData: {
              round: 1,
              showTooltip: true,
              onDragEnd: (_e, datasetIndex, index, value) => {
                (chart!.data.datasets[datasetIndex].data[index] as any).y = value;
                const updated = chart!.data.datasets[0].data.map(p => ({
                  temp: (p as any).x,
                  pwm: (p as any).y
                }));
                
                // When a point is dragged, update to custom preset
                selectedPreset = 'custom';
                presets.custom = updated;
                
                dispatch('update', updated);
              }
            }
          }
        }
      });
    });
  
    onDestroy(() => {
      chart?.destroy();
    });
</script>

<div class="flex flex-col gap-4">
  <div class="relative w-full h-64 bg-gray-900 rounded-xl p-4 shadow-lg ring-1 ring-cyan-900/20">
    <canvas bind:this={canvasEl} class="w-full h-full"></canvas>
  </div>
  
  <div class="flex flex-col sm:flex-row gap-2 justify-between bg-gray-900 rounded-xl p-4 shadow-lg ring-1 ring-cyan-900/20">
    <div class="text-cyan-400 font-semibold">Fan Curve Presets:</div>
    <div class="flex gap-2">
      <button 
        class="px-3 py-1 rounded-md text-sm {selectedPreset === 'silent' ? 'bg-cyan-800 text-white' : 'bg-gray-800 text-gray-200 hover:bg-gray-700'}"
        on:click={() => applyPreset('silent')}>
        Silent
      </button>
      <button 
        class="px-3 py-1 rounded-md text-sm {selectedPreset === 'balanced' ? 'bg-cyan-800 text-white' : 'bg-gray-800 text-gray-200 hover:bg-gray-700'}"
        on:click={() => applyPreset('balanced')}>
        Balanced
      </button>
      <button 
        class="px-3 py-1 rounded-md text-sm {selectedPreset === 'performance' ? 'bg-cyan-800 text-white' : 'bg-gray-800 text-gray-200 hover:bg-gray-700'}"
        on:click={() => applyPreset('performance')}>
        Performance
      </button>
      <button 
        class="px-3 py-1 rounded-md text-sm {selectedPreset === 'custom' ? 'bg-cyan-800 text-white' : 'bg-gray-800 text-gray-200 hover:bg-gray-700'}"
        on:click={() => applyPreset('custom')}>
        Custom
      </button>
    </div>
  </div>
  
  <div class="text-gray-400 text-sm">
    <p>Drag points on the chart to adjust the fan curve, or select a preset.</p>
    <p>Changes from manual tuning will be automatically synchronized with the curve.</p>
  </div>
</div>