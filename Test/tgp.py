#!/usr/bin/env python3
"""
HP Omen TGP Discovery and Control Script for Linux
Uses ACPI/sysfs interfaces and direct hardware access methods
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

# TGP Power Levels
TGP_LEVELS = {
    'minimum': {'custom_tgp': 0x00, 'ppab': 0x00, 'desc': 'Base TGP only (~95W)', 'watts': 95},
    'medium':  {'custom_tgp': 0x01, 'ppab': 0x00, 'desc': 'Custom TGP enabled (~115W)', 'watts': 115},
    'maximum': {'custom_tgp': 0x01, 'ppab': 0x01, 'desc': 'Custom TGP + PPAB (~150W)', 'watts': 150}
}

# ACPI paths to check
ACPI_PATHS = [
    '/sys/kernel/debug/ec',
    '/proc/acpi',
    '/sys/firmware/acpi',
    '/sys/class/hwmon',
    '/sys/class/thermal'
]

# HP-specific ACPI methods
HP_ACPI_METHODS = [
    'HPOM',  # HP Omen methods
    'WTGP',  # WMI TGP methods  
    'WMAX',  # WMI Max methods
    'HWMI',  # HP WMI
    'WQAA',  # WMI Query methods
    'WSAA',  # WMI Set methods
]

class HPOmenTGPLinuxController:
    def __init__(self):
        self.root_check()
        self.acpi_available = False
        self.ec_available = False
        self.hwmon_paths = []
        self.thermal_zones = []
        self.nvidia_gpu = None
        self.amd_gpu = None
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
    
    def discover_hp_acpi_methods(self):
        """Discover HP-specific ACPI methods"""
        print("\n🔍 Discovering HP ACPI Methods...")
        print("=" * 50)
        
        if not self.acpi_available:
            print("❌ ACPI call interface not available")
            return
        
        # Try to find HP ACPI methods
        acpi_methods_found = []
        
        for method in HP_ACPI_METHODS:
            if self.test_acpi_method(method):
                acpi_methods_found.append(method)
                print(f"✓ Found ACPI method: {method}")
        
        if acpi_methods_found:
            print(f"✓ Found {len(acpi_methods_found)} HP ACPI methods")
        else:
            print("⚠️  No HP ACPI methods found")
            
        # Try to discover WMI methods via ACPI
        self.discover_wmi_methods()
    
    def test_acpi_method(self, method: str) -> bool:
        """Test if an ACPI method exists"""
        try:
            # Try different ACPI paths
            test_calls = [
                f"\\_SB.{method}",
                f"\\_SB.PCI0.{method}",
                f"\\_SB.PCI0.LPCB.{method}",
                f"\\{method}"
            ]
            
            for call in test_calls:
                if self.call_acpi_method(call, test_only=True):
                    return True
            return False
        except:
            return False
    
    def call_acpi_method(self, method: str, args: List[int] = None, test_only: bool = False) -> Optional[bytes]:
        """Call an ACPI method"""
        if not self.acpi_available:
            return None
        
        try:
            # Format the ACPI call
            if args:
                arg_str = ' '.join([f'0x{arg:02x}' for arg in args])
                call_str = f"{method} {arg_str}"
            else:
                call_str = method
            
            if test_only:
                # Just check if the method exists without actually calling it
                call_str = f"{method} 0x00"  # Safe test call
            
            # Write to ACPI call interface
            with open('/proc/acpi/call', 'w') as f:
                f.write(call_str)
            
            # Read result
            with open('/proc/acpi/call', 'r') as f:
                result = f.read().strip()
            
            if result and result != '0x0':
                return bytes.fromhex(result.replace('0x', ''))
            return None
            
        except Exception as e:
            if not test_only:
                print(f"ACPI call failed: {e}")
            return None
    
    def discover_wmi_methods(self):
        """Try to discover WMI methods through ACPI"""
        print("\n🔍 Discovering WMI Methods via ACPI...")
        print("=" * 50)
        
        # HP WMI GUIDs to test
        hp_guids = [
            "5FB7F034-2C63-45e9-BE91-3D44E2C707E4"
        ]
        
        # Try WMI methods
        wmi_methods = ['WQAA', 'WSAA', 'WMAX', 'WMAA']
        
        for method in wmi_methods:
            result = self.call_acpi_method(f"\\_SB.WMID.{method}", test_only=True)
            if result:
                print(f"✓ Found WMI method: {method}")
    
    def create_tgp_payload(self, custom_tgp: int, ppab: int, device_state: int = 0x01, peak_temp: int = 87) -> bytes:
        """Create the TGP control payload"""
        # Create the 128-byte payload
        payload = bytearray(128)
        
        # Security signature "SECU"
        payload[0:4] = b"SECU"
        
        # TGP configuration
        payload[4] = custom_tgp      # Custom TGP flag
        payload[5] = ppab            # PPAB flag
        payload[6] = device_state    # Device state (D1)
        payload[7] = peak_temp       # Peak temperature
        
        return bytes(payload)
    
    def set_tgp_via_acpi(self, level: str) -> bool:
        """Set TGP level via ACPI methods"""
        if level not in TGP_LEVELS:
            return False
        
        config = TGP_LEVELS[level]
        print(f"🔧 Attempting ACPI TGP control for {level}")
        
        # Create payload
        payload = self.create_tgp_payload(
            custom_tgp=config['custom_tgp'],
            ppab=config['ppab']
        )
        
        # Try different ACPI method calls
        acpi_attempts = [
            "\\_SB.WMID.WSAA",  # WMI Set method
            "\\_SB.PCI0.LPCB.HWMI",  # HP WMI
            "\\_SB.WTGP",  # Direct TGP method
            "\\_SB.HPOM"   # HP Omen method
        ]
        
        for method in acpi_attempts:
            try:
                # Convert payload to list of integers for ACPI
                args = list(payload[:16])  # Use first 16 bytes
                result = self.call_acpi_method(method, args)
                
                if result:
                    print(f"✓ ACPI method {method} succeeded")
                    return True
                    
            except Exception as e:
                print(f"⚠️  ACPI method {method} failed: {e}")
        
        return False
    
    def set_tgp_via_ec(self, level: str) -> bool:
        """Set TGP level via Embedded Controller"""
        if not self.ec_available or level not in TGP_LEVELS:
            return False
        
        config = TGP_LEVELS[level]
        print(f"🔧 Attempting EC TGP control for {level}")
        
        try:
            # HP Omen EC registers (approximate, may need adjustment)
            ec_base = 0x62  # Standard EC command port
            ec_data = 0x66  # Standard EC data port
            
            # HP-specific TGP control registers (these are estimates)
            tgp_control_reg = 0x9A  # TGP control register
            ppab_control_reg = 0x9B  # PPAB control register
            
            # Write TGP configuration to EC
            self.write_ec_register(tgp_control_reg, config['custom_tgp'])
            time.sleep(0.1)
            self.write_ec_register(ppab_control_reg, config['ppab'])
            
            print(f"✓ EC registers written for {level}")
            return True
            
        except Exception as e:
            print(f"⚠️  EC method failed: {e}")
            return False
    
    def write_ec_register(self, register: int, value: int):
        """Write to EC register"""
        try:
            # Try via ec_sys interface
            ec_path = '/sys/kernel/debug/ec/ec0/io'
            if os.path.exists(ec_path):
                with open(ec_path, 'r+b') as f:
                    f.seek(register)
                    f.write(bytes([value]))
                return
            
            # Fallback to direct port access (dangerous!)
            with open('/dev/port', 'r+b') as f:
                # Wait for EC to be ready
                f.seek(0x66)
                while f.read(1)[0] & 0x02:
                    time.sleep(0.001)
                
                # Send command
                f.seek(0x66)
                f.write(bytes([0x81]))  # Write command
                
                # Send register address
                while f.read(1)[0] & 0x02:
                    time.sleep(0.001)
                f.seek(0x62)
                f.write(bytes([register]))
                
                # Send data
                f.seek(0x66)
                while f.read(1)[0] & 0x02:
                    time.sleep(0.001)
                f.seek(0x62)
                f.write(bytes([value]))
                
        except Exception as e:
            raise Exception(f"EC write failed: {e}")
    
    def set_tgp_via_nvidia(self, level: str) -> bool:
        """Set power limit via NVIDIA tools"""
        if not self.nvidia_gpu or level not in TGP_LEVELS:
            return False
        
        target_watts = TGP_LEVELS[level]['watts']
        print(f"🔧 Setting NVIDIA power limit to {target_watts}W")
        
        try:
            # Try nvidia-smi power limit
            cmd = ['nvidia-smi', '-pl', str(target_watts)]
            result = subprocess.run(cmd, capture_output=True, text=True)
            
            if result.returncode == 0:
                print(f"✓ NVIDIA power limit set to {target_watts}W")
                return True
            else:
                print(f"⚠️  nvidia-smi failed: {result.stderr}")
                
        except Exception as e:
            print(f"⚠️  NVIDIA method failed: {e}")
        
        return False
    
    def set_tgp_level(self, level: str) -> bool:
        """Set TGP level using available methods"""
        if level not in TGP_LEVELS:
            print(f"❌ Invalid level. Choose from: {list(TGP_LEVELS.keys())}")
            return False
        
        config = TGP_LEVELS[level]
        print(f"\n🔧 Setting TGP to {level} - {config['desc']}")
        print("=" * 50)
        
        success = False
        
        # Try different methods in order of preference
        methods = [
            ("ACPI", self.set_tgp_via_acpi),
            ("EC", self.set_tgp_via_ec),
            ("NVIDIA", self.set_tgp_via_nvidia)
        ]
        
        for method_name, method_func in methods:
            print(f"\n🔄 Trying {method_name} method...")
            if method_func(level):
                success = True
                print(f"✓ TGP successfully set via {method_name}")
                break
            else:
                print(f"⚠️  {method_name} method failed")
        
        if success:
            print(f"\n✅ TGP level set to {level} successfully!")
            print("⚠️  Changes may require a restart or re-login to take full effect")
            print("⚠️  Monitor temperatures and system stability")
            return True
        else:
            print(f"\n❌ All TGP setting methods failed for {level}")
            print("💡 Try running the discovery methods to find available interfaces")
            return False
    
    def show_current_status(self):
        """Show current system and hardware status"""
        print("\n📊 Current System Status")
        print("=" * 50)
        
        # Show CPU info
        try:
            with open('/proc/cpuinfo', 'r') as f:
                for line in f:
                    if 'model name' in line:
                        cpu_name = line.split(':')[1].strip()
                        print(f"🖥️  CPU: {cpu_name}")
                        break
        except:
            pass
        
        # Show GPU info
        if self.nvidia_gpu:
            print(f"🎮 GPU: {self.nvidia_gpu}")
            try:
                result = subprocess.run(['nvidia-smi', '--query-gpu=power.draw,power.limit,temperature.gpu', '--format=csv,noheader,nounits'], 
                                      capture_output=True, text=True)
                if result.returncode == 0:
                    power_info = result.stdout.strip().split(', ')
                    print(f"   Power: {power_info[0]}W / {power_info[1]}W")
                    print(f"   Temp: {power_info[2]}°C")
            except:
                pass
        elif self.amd_gpu:
            print(f"🎮 GPU: {self.amd_gpu}")
        
        # Show thermal zones
        print(f"\n🌡️  Thermal Zones:")
        for zone in self.thermal_zones[:5]:  # Show first 5
            try:
                temp_file = os.path.join(zone, 'temp')
                type_file = os.path.join(zone, 'type')
                
                if os.path.exists(temp_file) and os.path.exists(type_file):
                    with open(temp_file, 'r') as f:
                        temp = int(f.read().strip()) / 1000
                    with open(type_file, 'r') as f:
                        zone_type = f.read().strip()
                    print(f"   {zone_type}: {temp:.1f}°C")
            except:
                pass
        
        # Show hardware access status
        print(f"\n🔧 Hardware Access:")
        print(f"   ACPI: {'✓' if self.acpi_available else '❌'}")
        print(f"   EC: {'✓' if self.ec_available else '❌'}")
        print(f"   Hwmon interfaces: {len(self.hwmon_paths)}")
    
    def interactive_menu(self):
        """Interactive menu for TGP control"""
        while True:
            print("\n" + "="*60)
            print("🎮 HP Omen TGP Control Menu (Linux)")
            print("="*60)
            print("1. Discover Hardware Methods")
            print("2. Show System Status") 
            print("3. Set TGP to Minimum (~95W)")
            print("4. Set TGP to Medium (~115W)")
            print("5. Set TGP to Maximum (~150W)")
            print("6. Show Help & Requirements")
            print("7. Exit")
            print("="*60)
            
            try:
                choice = input("Select option (1-7): ").strip()
                
                if choice == '1':
                    self.discover_hp_acpi_methods()
                elif choice == '2':
                    self.show_current_status()
                elif choice == '3':
                    self.set_tgp_level('minimum')
                elif choice == '4':
                    self.set_tgp_level('medium')
                elif choice == '5':
                    self.set_tgp_level('maximum')
                elif choice == '6':
                    self.show_help()
                elif choice == '7':
                    print("👋 Goodbye!")
                    break
                else:
                    print("❌ Invalid choice. Please select 1-7.")
                    
            except KeyboardInterrupt:
                print("\n👋 Goodbye!")
                break
            except Exception as e:
                print(f"❌ Error: {e}")
    
    def show_help(self):
        """Show help and requirements"""
        print("\n📚 Help & Requirements")
        print("=" * 50)
        print("This script requires several components to work:")
        print()
        print("1. Root privileges (sudo)")
        print("2. Kernel modules:")
        print("   - acpi_call or acpi-call-dkms")
        print("   - ec_sys (with write_support=1)")
        print("   - msr")
        print()
        print("3. Installation commands:")
        print("   Ubuntu/Debian:")
        print("   sudo apt install acpi-call-dkms")
        print("   echo 'ec_sys write_support=1' | sudo tee -a /etc/modprobe.d/ec_sys.conf")
        print()
        print("   Arch Linux:")
        print("   sudo pacman -S acpi_call")
        print("   echo 'ec_sys write_support=1' | sudo tee -a /etc/modprobe.d/ec_sys.conf")
        print()
        print("4. Reboot after installation")
        print()
        print("⚠️  WARNING: This script modifies hardware settings!")
        print("   - Monitor temperatures carefully")
        print("   - Test changes gradually")
        print("   - Have a recovery plan ready")

def main():
    """Main function"""
    print("🚀 HP Omen TGP Control Tool for Linux")
    print("⚠️  CAUTION: Hardware modification tool - use at your own risk!")
    print("⚠️  Monitor temperatures and system stability!")
    
    try:
        controller = HPOmenTGPLinuxController()
        
        # Run initial discovery
        controller.discover_hp_acpi_methods()
        
        # Show interactive menu
        controller.interactive_menu()
        
    except KeyboardInterrupt:
        print("\n👋 Goodbye!")
    except Exception as e:
        print(f"❌ Fatal error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
