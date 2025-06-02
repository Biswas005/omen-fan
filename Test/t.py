#!/usr/bin/env python3
"""
HP Omen TGP Discovery and Control Script for Linux - Enhanced ACPI Discovery
Uses extensive WMI and ACPI device enumeration to find HP Omen methods
"""

import os
import sys
import struct
import subprocess
import glob
import json
from typing import List, Optional, Dict, Any, Union
from pathlib import Path
import time
import re

# TGP Power Levels (from BiosData.cs GpuPowerLevel enum)
TGP_LEVELS = {
    'minimum': {'custom_tgp': 0x00, 'ppab': 0x00, 'desc': 'Base TGP only', 'level': 0x00},
    'medium':  {'custom_tgp': 0x01, 'ppab': 0x00, 'desc': 'Custom TGP enabled', 'level': 0x01},
    'maximum': {'custom_tgp': 0x01, 'ppab': 0x01, 'desc': 'Custom TGP + PPAB', 'level': 0x02}
}

# Fan Modes (from BiosData.cs FanMode enum)
FAN_MODES = {
    'default': 0x30,        # 48 - Default
    'performance': 0x31,    # 49 - Performance
    'cool': 0x50,          # 80 - Cool
    'legacy_quiet': 0x03,   # 3 - Legacy Quiet
    'legacy_extreme': 0x04  # 4 - Legacy Extreme
}

# Command identifiers (from BiosData.cs Cmd enum)
BIOS_COMMANDS = {
    'default': 0x20008,    # 131080 - Most commands
    'keyboard': 0x20009,   # 131081 - Keyboard-related
    'legacy': 0x00001,     # 1 - Earliest implemented
    'gpu_mode': 0x00002    # 2 - Graphics mode switch
}

# WMI BIOS constants (from BiosData.cs)
WMI_CONSTANTS = {
    'bios_data': 'hpqBDataIn',
    'bios_data_field': 'hpqBData',
    'bios_method': 'hpqBIOSInt',
    'bios_method_class': 'hpqBIntM',
    'bios_method_instance': 'ACPI\\PNP0C14\\0_0',
    'bios_namespace': 'root\\wmi',
    'return_code_field': 'rwReturnCode'
}

# Authorization signature (from BiosData.cs Sign array)
BIOS_SIGNATURE = [0x53, 0x45, 0x43, 0x55]  # "SECU"

# Target ACPI/WMI Device IDs to search for
TARGET_DEVICE_IDS = [
    "PNP0C14",  # Windows Management Instrumentation for ACPI
    "HPQ6001",  # HP Hotkey
    "HPQ6007",  # HP WMI
    "ACPI0012", # ACPI Generic Event Device
    "HPQ0004",  # HP System Device
    "HPQ0006",  # HP WMI Hotkey
    "HPQ0068",  # HP WMI
    "INT33A0",  # Intel WMI Device
    "WMID",     # Generic WMI Device
    "WMAA",     # WMI Method Alias
    "WQAA",     # WMI Query All
    "WSAA"      # WMI Set All
]

# Common ACPI method names to search for in devices
ACPI_METHOD_NAMES = [
    "WQAA", "WSAA", "WMAA", "WBAA", "WOAA", "WRAA",  # WMI methods
    "HPQI", "HPQB", "HWMI", "HKEY", "HOTK",          # HP methods
    "BIOS", "WMID", "OMEM", "OMEN", "QMOD",          # Omen/BIOS methods
    "GPWR", "TGPX", "FANS", "COOL", "PERF",          # Hardware control
    "INIT", "STAT", "INFO", "CTRL", "SETM", "GETM"   # Generic control
]

# ACPI paths to check
ACPI_PATHS = [
    '/sys/kernel/debug/ec',
    '/proc/acpi',
    '/sys/firmware/acpi',
    '/sys/class/hwmon',
    '/sys/class/thermal'
]

class HPOmenBiosController:
    def __init__(self):
        self.root_check()
        self.acpi_available = False
        self.ec_available = False
        self.hwmon_paths = []
        self.thermal_zones = []
        self.nvidia_gpu = None
        self.amd_gpu = None
        self.found_methods = {}
        self.discovered_devices = {}
        self.acpi_namespace = {}
        self.setup_hardware_access()
    
    def root_check(self):
        """Check if running as root (required for hardware access)"""
        if os.geteuid() != 0:
            print("❌ This script requires root privileges for hardware access")
            print("   Please run with: sudo python3 script.py")
            sys.exit(1)
        print("✓ Running with root privileges")
    
    def setup_hardware_access(self):
        """Initialize hardware access methods"""
        print("\n🔍 Setting up hardware access...")
        print("=" * 50)
        
        # Check ACPI availability
        self.check_acpi_access()
        
        # Check EC (Embedded Controller) access
        self.check_ec_access()
        
        # Find hardware monitoring interfaces
        self.find_hwmon_interfaces()
        
        # Find thermal zones
        self.find_thermal_zones()
        
        # Detect GPU
        self.detect_gpu()
        
        # Try to load necessary kernel modules
        self.load_kernel_modules()
    
    def check_acpi_access(self):
        """Check ACPI interface availability"""
        acpi_found = False
        
        for path in ACPI_PATHS:
            if os.path.exists(path):
                print(f"✓ Found ACPI path: {path}")
                acpi_found = True
        
        # Check for ACPI call interface
        acpi_call_paths = [
            '/proc/acpi/call',
            '/sys/kernel/debug/acpi/custom_method'
        ]
        
        for path in acpi_call_paths:
            if os.path.exists(path):
                print(f"✓ Found ACPI call interface: {path}")
                self.acpi_available = True
                break
        
        if not self.acpi_available:
            print("⚠️  ACPI call interface not found")
            print("   Try installing: acpi-call-dkms or acpi_call kernel module")
    
    def check_ec_access(self):
        """Check Embedded Controller access"""
        ec_paths = [
            '/sys/kernel/debug/ec/ec0',
            '/dev/port'  # Raw port access fallback
        ]
        
        for path in ec_paths:
            if os.path.exists(path):
                print(f"✓ Found EC access: {path}")
                self.ec_available = True
                break
        
        if not self.ec_available:
            print("⚠️  EC access not available")
            print("   Enable with: echo 'module ec_sys write_support=1' >> /etc/modprobe.d/ec_sys.conf")
    
    def find_hwmon_interfaces(self):
        """Find hardware monitoring interfaces"""
        hwmon_base = '/sys/class/hwmon'
        if os.path.exists(hwmon_base):
            self.hwmon_paths = glob.glob(f"{hwmon_base}/hwmon*")
            print(f"✓ Found {len(self.hwmon_paths)} hwmon interfaces")
            
            for path in self.hwmon_paths:
                try:
                    name_file = os.path.join(path, 'name')
                    if os.path.exists(name_file):
                        with open(name_file, 'r') as f:
                            name = f.read().strip()
                            print(f"  - {path}: {name}")
                except:
                    pass
    
    def find_thermal_zones(self):
        """Find thermal zones"""
        thermal_base = '/sys/class/thermal'
        if os.path.exists(thermal_base):
            self.thermal_zones = glob.glob(f"{thermal_base}/thermal_zone*")
            print(f"✓ Found {len(self.thermal_zones)} thermal zones")
    
    def detect_gpu(self):
        """Detect GPU and its control interfaces"""
        try:
            # Check for NVIDIA GPU
            result = subprocess.run(['lspci'], capture_output=True, text=True)
            if result.returncode == 0:
                for line in result.stdout.split('\n'):
                    if 'nvidia' in line.lower() or 'geforce' in line.lower():
                        print(f"🎮 Found NVIDIA GPU: {line.strip()}")
                        self.nvidia_gpu = line.strip()
                    elif 'amd' in line.lower() or 'radeon' in line.lower():
                        print(f"🎮 Found AMD GPU: {line.strip()}")
                        self.amd_gpu = line.strip()
            
            # Check for nvidia-smi
            if self.nvidia_gpu:
                result = subprocess.run(['nvidia-smi', '--query-gpu=name,power.draw,power.limit', '--format=csv,noheader,nounits'], 
                                      capture_output=True, text=True)
                if result.returncode == 0:
                    print(f"✓ NVIDIA tools available")
                    print(f"  GPU info: {result.stdout.strip()}")
                    
        except Exception as e:
            print(f"⚠️  GPU detection failed: {e}")
    
    def load_kernel_modules(self):
        """Load necessary kernel modules"""
        modules = ['acpi_call', 'ec_sys', 'msr']
        
        for module in modules:
            try:
                result = subprocess.run(['modprobe', module], capture_output=True)
                if result.returncode == 0:
                    print(f"✓ Loaded kernel module: {module}")
                else:
                    print(f"⚠️  Failed to load module {module}")
            except:
                print(f"⚠️  Could not load module {module}")
    
    def enumerate_acpi_devices(self):
        """Enumerate all ACPI devices and build namespace map"""
        print("\n🔍 Enumerating ACPI devices...")
        print("=" * 50)
        
        devices_found = {}
        
        # Method 1: Parse /sys/bus/acpi/devices
        acpi_devices_path = '/sys/bus/acpi/devices'
        if os.path.exists(acpi_devices_path):
            print("📁 Scanning /sys/bus/acpi/devices...")
            for device_dir in os.listdir(acpi_devices_path):
                device_path = os.path.join(acpi_devices_path, device_dir)
                if os.path.isdir(device_path):
                    device_info = self.parse_acpi_device(device_path, device_dir)
                    if device_info:
                        devices_found[device_dir] = device_info
        
        # Method 2: Parse /proc/acpi
        proc_acpi_path = '/proc/acpi'
        if os.path.exists(proc_acpi_path):
            print("📁 Scanning /proc/acpi...")
            for item in os.listdir(proc_acpi_path):
                item_path = os.path.join(proc_acpi_path, item)
                if os.path.isdir(item_path):
                    device_info = self.parse_proc_acpi_device(item_path, item)
                    if device_info:
                        devices_found[f"proc_{item}"] = device_info
        
        # Method 3: Use acpi command if available
        try:
            result = subprocess.run(['acpi', '-V'], capture_output=True, text=True)
            if result.returncode == 0:
                print("📋 ACPI command output:")
                for line in result.stdout.split('\n'):
                    if line.strip():
                        print(f"   {line}")
        except:
            pass
        
        self.discovered_devices = devices_found
        
        # Filter for target devices
        target_devices = {}
        for device_id, device_info in devices_found.items():
            for target_id in TARGET_DEVICE_IDS:
                if target_id in device_info.get('hid', '') or target_id in device_info.get('path', ''):
                    target_devices[device_id] = device_info
                    print(f"🎯 Found target device: {device_id} -> {device_info}")
        
        print(f"\n📊 Summary:")
        print(f"   Total ACPI devices found: {len(devices_found)}")
        print(f"   Target devices found: {len(target_devices)}")
        
        return target_devices
    
    def parse_acpi_device(self, device_path: str, device_id: str) -> Dict[str, Any]:
        """Parse ACPI device information from sysfs"""
        device_info = {'device_id': device_id, 'path': device_path}
        
        # Read device properties
        property_files = ['hid', 'uid', '_HID', '_UID', 'modalias', 'description']
        for prop_file in property_files:
            prop_path = os.path.join(device_path, prop_file)
            if os.path.exists(prop_path):
                try:
                    with open(prop_path, 'r') as f:
                        content = f.read().strip()
                        device_info[prop_file] = content
                except:
                    pass
        
        # Check for power management
        power_path = os.path.join(device_path, 'power')
        if os.path.exists(power_path):
            device_info['has_power_mgmt'] = True
        
        # Check for firmware node
        firmware_path = os.path.join(device_path, 'firmware_node')
        if os.path.exists(firmware_path):
            device_info['has_firmware_node'] = True
        
        return device_info
    
    def parse_proc_acpi_device(self, device_path: str, device_name: str) -> Dict[str, Any]:
        """Parse ACPI device information from /proc/acpi"""
        device_info = {'device_name': device_name, 'path': device_path}
        
        # Look for info file
        info_file = os.path.join(device_path, 'info')
        if os.path.exists(info_file):
            try:
                with open(info_file, 'r') as f:
                    content = f.read()
                    device_info['info'] = content
            except:
                pass
        
        return device_info
    
    def build_acpi_namespace_map(self):
        """Build comprehensive ACPI namespace map"""
        print("\n🗺️  Building ACPI namespace map...")
        print("=" * 50)
        
        # Start with common ACPI paths
        base_paths = [
            "\\_SB",
            "\\_SB.PCI0",
            "\\_SB.PCI0.LPCB",
            "\\_SB.PCI0.LPC",
            "\\_SB.WMI0",
            "\\_SB.WMI1",
            "\\_SB.WMID",
            "\\_SB.WMAA",
            "\\_SB.WQAA",
            "\\_SB.WSAA"
        ]
        
        # Add device-specific paths based on discovered devices
        for device_id, device_info in self.discovered_devices.items():
            # Create potential ACPI paths for this device
            if 'hid' in device_info:
                hid = device_info['hid']
                for target_id in TARGET_DEVICE_IDS:
                    if target_id in hid:
                        # Generate possible paths
                        potential_paths = [
                            f"\\_SB.{target_id}",
                            f"\\_SB.PCI0.{target_id}",
                            f"\\_SB.PCI0.LPCB.{target_id}",
                            f"\\_SB.PCI0.LPC.{target_id}",
                            f"\\_SB.WMI0.{target_id}",
                            f"\\_SB.WMI1.{target_id}"
                        ]
                        base_paths.extend(potential_paths)
        
        # Generate method paths for each base path
        method_paths = []
        for base_path in base_paths:
            for method_name in ACPI_METHOD_NAMES:
                method_path = f"{base_path}.{method_name}"
                method_paths.append(method_path)
        
        # Remove duplicates and sort
        method_paths = sorted(list(set(method_paths)))
        
        print(f"🔍 Generated {len(method_paths)} potential ACPI method paths")
        
        # Also add EC-based paths
        ec_paths = []
        ec_bases = [
            "\\_SB.PCI0.LPCB.EC0",
            "\\_SB.PCI0.LPCB.H_EC",
            "\\_SB.PCI0.LPC.EC0",
            "\\_SB.PCI0.LPC.H_EC",
            "\\_SB.EC0",
            "\\_SB.H_EC"
        ]
        
        for ec_base in ec_bases:
            for method_name in ACPI_METHOD_NAMES:
                ec_paths.append(f"{ec_base}.{method_name}")
        
        method_paths.extend(ec_paths)
        method_paths = sorted(list(set(method_paths)))
        
        print(f"🔍 Total ACPI methods to test: {len(method_paths)}")
        self.acpi_namespace = {'methods': method_paths, 'devices': self.discovered_devices}
        
        return method_paths
    
    def discover_hp_bios_methods(self):
        """Enhanced HP BIOS method discovery"""
        print("\n🔍 Enhanced HP BIOS Method Discovery...")
        print("=" * 70)
        
        if not self.acpi_available:
            print("❌ ACPI call interface not available")
            return
        
        # Step 1: Enumerate ACPI devices
        target_devices = self.enumerate_acpi_devices()
        
        # Step 2: Build comprehensive namespace map
        method_paths = self.build_acpi_namespace_map()
        
        # Step 3: Test all generated method paths
        print(f"\n🧪 Testing {len(method_paths)} ACPI method paths...")
        print("-" * 70)

        # Save all method paths to a text file
        with open("tested_methods.txt", "w") as f:
            for method in method_paths:
                f.write(method + "\n")
        print(f"📁 Saved tested method paths to tested_methods.txt")

        
        working_methods = {}
        tested_count = 0
        
        for method_path in method_paths:
            tested_count += 1
            if tested_count % 20 == 0:
                print(f"   Progress: {tested_count}/{len(method_paths)} methods tested...")
            
            if self.test_acpi_method(method_path):
                working_methods[method_path] = "Available"
                print(f"✅ WORKING: {method_path}")
            else:
                # Don't spam output for non-working methods
                pass
        
        self.found_methods = working_methods
        
        # Step 4: Categorize discovered methods
        print(f"\n📊 Discovery Results:")
        print("=" * 50)
        print(f"Total methods tested: {len(method_paths)}")
        print(f"Working methods found: {len(working_methods)}")
        
        if working_methods:
            # Categorize by type
            wmi_methods = [m for m in working_methods.keys() if any(wmi in m for wmi in ['WQAA', 'WSAA', 'WMAA', 'WMI'])]
            hp_methods = [m for m in working_methods.keys() if any(hp in m for hp in ['HPQ', 'HPQI', 'HPQB', 'HOTK'])]
            ec_methods = [m for m in working_methods.keys() if 'EC' in m]
            other_methods = [m for m in working_methods.keys() if m not in wmi_methods + hp_methods + ec_methods]
            
            print(f"\n🔧 Method Categories:")
            if wmi_methods:
                print(f"   WMI Methods ({len(wmi_methods)}):")
                for method in wmi_methods[:10]:  # Show first 10
                    print(f"     ✓ {method}")
                if len(wmi_methods) > 10:
                    print(f"     ... and {len(wmi_methods) - 10} more")
            
            if hp_methods:
                print(f"   HP-Specific Methods ({len(hp_methods)}):")
                for method in hp_methods:
                    print(f"     ✓ {method}")
            
            if ec_methods:
                print(f"   EC Methods ({len(ec_methods)}):")
                for method in ec_methods:
                    print(f"     ✓ {method}")
            
            if other_methods:
                print(f"   Other Methods ({len(other_methods)}):")
                for method in other_methods[:5]:  # Show first 5
                    print(f"     ✓ {method}")
                if len(other_methods) > 5:
                    print(f"     ... and {len(other_methods) - 5} more")
        else:
            print("❌ No working HP BIOS methods found")
            print("💡 This may not be an HP Omen laptop or ACPI access is restricted")
    
    def test_acpi_method(self, method: str) -> bool:
        """Test if an ACPI method exists and responds"""
        try:
            # Try a safe query first (just checking existence)
            result = self.call_acpi_method(method, test_only=True)
            return result is not None
        except:
            return False
    
    def call_acpi_method(self, method: str, args: List[int] = None, test_only: bool = False) -> Optional[bytes]:
        """Call an ACPI method with enhanced error handling"""
        if not self.acpi_available:
            return None
        
        try:
            # For HP WMI methods, we need to format calls specifically
            if any(wmi in method for wmi in ['WMID', 'WMI', 'WQAA', 'WSAA', 'WMAA']):
                # WMI methods typically expect a buffer and instance ID
                if test_only:
                    call_str = f"{method} 0x00 0x00"  # Safe test parameters
                else:
                    if args:
                        # Format as WMI buffer call
                        buffer_args = ' '.join([f'0x{arg:02x}' for arg in args[:16]])
                        call_str = f"{method} {buffer_args}"
                    else:
                        call_str = f"{method} 0x00"
            else:
                # Standard ACPI method call
                if args:
                    arg_str = ' '.join([f'0x{arg:02x}' for arg in args])
                    call_str = f"{method} {arg_str}"
                else:
                    call_str = method if not test_only else f"{method} 0x00"
            
            # Write to ACPI call interface
            with open('/proc/acpi/call', 'w') as f:
                f.write(call_str)
            
            # Read result
            with open('/proc/acpi/call', 'r') as f:
                result = f.read().strip()
            
            if result and result != '0x0' and not result.startswith('Error'):
                # Try to convert hex result to bytes
                try:
                    hex_str = result.replace('0x', '').replace(' ', '')
                    if len(hex_str) % 2 == 0:
                        return bytes.fromhex(hex_str)
                    else:
                        return result.encode()  # Return as string if not valid hex
                except:
                    return result.encode()  # Return as string if hex conversion fails
            return None
            
        except Exception as e:
            if not test_only:
                print(f"ACPI call failed for {method}: {e}")
            return None
    
    def create_hp_bios_payload(self, command: int, data: List[int]) -> List[int]:
        """Create HP BIOS payload with signature and command structure"""
        # Start with the HP signature
        payload = list(BIOS_SIGNATURE)  # [0x53, 0x45, 0x43, 0x55] = "SECU"
        
        # Add command identifier
        payload.extend([
            (command >> 0) & 0xFF,
            (command >> 8) & 0xFF,
            (command >> 16) & 0xFF,
            (command >> 24) & 0xFF
        ])
        
        # Add data
        payload.extend(data)
        
        # Pad to appropriate length (HP expects specific buffer sizes)
        while len(payload) < 128:
            payload.append(0x00)
        
        return payload[:128]  # Ensure exactly 128 bytes
    
    def set_gpu_power_via_bios(self, level: str) -> bool:
        """Set GPU power using HP BIOS methods"""
        if level not in TGP_LEVELS or not self.found_methods:
            return False
        
        config = TGP_LEVELS[level]
        print(f"🔧 Setting GPU power via HP BIOS methods for level: {level}")
        
        # Create GPU power data structure (from BiosData.cs GpuPowerData)
        gpu_power_data = [
            config['custom_tgp'],  # CustomTgp
            config['ppab'],        # Ppab  
            0x01,                  # DState (D1)
            0x57                   # PeakTemperature (87°C = 0x57)
        ]
        
        # Create BIOS payload for GPU power control
        payload = self.create_hp_bios_payload(BIOS_COMMANDS['default'], gpu_power_data)
        
        # Try each found WMI method
        success = False
        for method in self.found_methods.keys():
            if any(set_method in method for set_method in ['WSAA', 'WMAA']):  # Set/Method operations
                try:
                    print(f"🔄 Trying BIOS method: {method}")
                    result = self.call_acpi_method(method, payload[:32])  # Use first 32 bytes
                    
                    if result:
                        print(f"✅ GPU power set successfully via {method}")
                        success = True
                        break
                    else:
                        print(f"⚠️  Method {method} returned no result")
                        
                except Exception as e:
                    print(f"⚠️  Method {method} failed: {e}")
        
        return success
    
    def set_fan_mode_via_bios(self, mode: str) -> bool:
        """Set fan mode using HP BIOS methods"""
        if mode not in FAN_MODES or not self.found_methods:
            return False
        
        fan_mode_value = FAN_MODES[mode]
        print(f"🔧 Setting fan mode to {mode} (0x{fan_mode_value:02x})")
        
        # Create fan control payload
        fan_data = [fan_mode_value, 0x00, 0x00, 0x00]  # Fan mode + padding
        payload = self.create_hp_bios_payload(BIOS_COMMANDS['default'], fan_data)
        
        # Try WMI set methods
        success = False
        for method in self.found_methods.keys():
            if 'WSAA' in method:
                try:
                    result = self.call_acpi_method(method, payload[:16])
                    if result:
                        print(f"✅ Fan mode set via {method}")
                        success = True
                        break
                except Exception as e:
                    print(f"⚠️  Fan control via {method} failed: {e}")
        
        return success
    
    def query_bios_info(self) -> Dict[str, Any]:
        """Query BIOS information using HP methods"""
        if not self.found_methods:
            return {}
        
        print("🔍 Querying HP BIOS information...")
        bios_info = {}
        
        # Try query methods
        for method in self.found_methods.keys():
            if 'WQAA' in method:  # Query All Attributes
                try:
                    # Query system information
                    result = self.call_acpi_method(method, [0x00])  # Query basic info
                    if result and len(result) >= 8:
                        # Parse basic system data (simplified)
                        bios_info[method] = {
                            'status_flags': int.from_bytes(result[0:2], 'little') if len(result) >= 2 else 0,
                            'thermal_policy': result[3] if len(result) > 3 else 0,
                            'support_flags': result[4] if len(result) > 4 else 0,
                            'raw_data': result.hex(),
                            'length': len(result)
                        }
                        print(f"✓ Got BIOS info from {method} ({len(result)} bytes)")
                except Exception as e:
                    print(f"⚠️  BIOS query via {method} failed: {e}")
        
        return bios_info
    
    def set_tgp_level(self, level: str) -> bool:
        """Set TGP level using HP BIOS methods"""
        if level not in TGP_LEVELS:
            print(f"❌ Invalid level. Choose from: {list(TGP_LEVELS.keys())}")
            return False
        
        config = TGP_LEVELS[level]
        print(f"\n🔧 Setting TGP to {level} - {config['desc']}")
        print("=" * 50)
        
        success = False
        
        # Primary method: HP BIOS GPU
        if self.set_gpu_power_via_bios(level):
            print(f"✓ TGP set to {level} via HP BIOS method")
            success = True
        else:
            print(f"⚠️  Failed to set TGP via HP BIOS method")
        
        # Optionally, try fallback or legacy methods here if needed
        # (e.g., EC direct, ACPI alternate, or vendor tools)
        
        return success

if __name__ == "__main__":
    controller = HPOmenBiosController()
    controller.discover_hp_bios_methods()
    # Example usage:
    # controller.set_tgp_level('maximum')
    # controller.set_fan_mode_via_bios('performance')
    # info = controller.query_bios_info()
    # print(json.dumps(info, indent=2))